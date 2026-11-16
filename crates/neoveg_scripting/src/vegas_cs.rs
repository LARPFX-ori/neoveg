//! Ponte gerenciada `ScriptPortal.Vegas` (Sala Limpa) e aplicador de mutações
//! no `EditorSession` sem uso de `unsafe`.

use crate::dotnet_host::{self, DotnetEnv};
use neoveg_common::error::NeovegError;
use neoveg_engine::commands::{run_command, CommandHost, HeadlessHost};
use neoveg_engine::model::{Marker, Track, TrackKind};
use neoveg_engine::session::{EditorSession, MAX_UNDO};
use neoveg_engine::time::Ticks;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const MAX_SCRIPT_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIAGNOSTIC_CHARS: usize = 1200;
const MAX_SCRIPT_ITEMS: usize = 4096;
static RUN_SEQ: AtomicU64 = AtomicU64::new(1);
static BRIDGE_DLL: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[derive(Serialize)]
struct ScriptTrackSnap {
    index: usize,
    name: String,
    is_video: bool,
    mute: bool,
    solo: bool,
}

#[derive(Serialize)]
struct ScriptMarkerSnap {
    position_ms: f64,
    label: String,
}

#[derive(Serialize)]
struct ScriptInputPayload {
    result_path: String,
    target_assembly_path: Option<String>,
    cursor_ms: f64,
    tracks: Vec<ScriptTrackSnap>,
    markers: Vec<ScriptMarkerSnap>,
    audio_buses: Vec<String>,
}

/// Versão do formato de saída da ponte. Uma saída sem ela veio de uma ponte
/// antiga (só "adicionados"): é recusada, nunca lida como "apagar tudo".
const SCRIPT_OUTPUT_PROTOCOL: u32 = 2;

/// Trilha na lista final devolvida pelo script: `orig` é o índice original
/// (ou negativo se é nova); `None` = não mudou.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptTrackOut {
    orig: i64,
    is_video: bool,
    name: Option<String>,
    mute: Option<bool>,
    solo: Option<bool>,
}

/// Marcador na lista final devolvida pelo script (mesma convenção).
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptMarkerOut {
    orig: i64,
    seconds: Option<f64>,
    label: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptOutputPayload {
    ok: bool,
    error: Option<String>,
    protocol: u32,
    messages: Vec<String>,
    commands: Vec<String>,
    tracks: Vec<ScriptTrackOut>,
    added_buses: Vec<Option<String>>,
    markers: Vec<ScriptMarkerOut>,
    cursor_ms: Option<f64>,
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

fn cs_project_xml(tfm: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{tfm}</TargetFramework>
    <OutputType>Library</OutputType>
    <AssemblyName>NeovegScript</AssemblyName>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>disable</Nullable>
    <ProduceReferenceAssembly>false</ProduceReferenceAssembly>
    <UseSharedCompilation>false</UseSharedCompilation>
  </PropertyGroup>
</Project>
"#
    )
}

const SCRIPT_PORTAL_VEGAS_CS: &str = r#"using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Text;
using System.Text.Json;

namespace ScriptPortal.Vegas
{
    public enum RulerFormat
    {
        Time,
        Seconds,
        TimeAndFrames,
        AbsoluteFrames,
        Samples
    }

    public enum MediaType
    {
        Unknown,
        Video,
        Audio
    }

    public sealed class Timecode
    {
        public double Milliseconds { get; set; }
        public Timecode() { }
        public Timecode(double ms) { Milliseconds = double.IsFinite(ms) ? ms : 0.0; }
        public static Timecode FromMilliseconds(double ms) => new Timecode(ms);
        public static Timecode FromSeconds(double s) => new Timecode(s * 1000.0);
        public double ToMilliseconds() => Milliseconds;
    }

    public class Marker
    {
        internal int _origIndex = -1;
        public Timecode Position { get; set; } = new Timecode(0.0);
        public string Label { get; set; } = "";
        public Marker() { }
        public Marker(Timecode pos, string label)
        {
            Position = pos ?? new Timecode(0.0);
            Label = label ?? "";
        }
    }

    public class Track
    {
        internal int _origIndex = -1;
        public int Index { get; internal set; }
        public string Name { get; set; } = "";
        public bool Mute { get; set; }
        public bool Solo { get; set; }
        public virtual bool IsVideo() => false;
        public virtual bool IsAudio() => false;
    }

    public sealed class VideoTrack : Track
    {
        public override bool IsVideo() => true;
    }

    public sealed class AudioTrack : Track
    {
        public override bool IsAudio() => true;
    }

    public sealed class TransportControl
    {
        public Timecode CursorPosition { get; set; } = new Timecode(0.0);
    }

    public sealed class Project
    {
        public List<Track> Tracks { get; } = new List<Track>();
        public List<Marker> Markers { get; } = new List<Marker>();
        public List<string> AudioBuses { get; } = new List<string>();

        public VideoTrack AddVideoTrack(string name = null)
        {
            var t = new VideoTrack { Index = Tracks.Count, Name = name ?? "" };
            Tracks.Add(t);
            return t;
        }

        public AudioTrack AddAudioTrack(string name = null)
        {
            var t = new AudioTrack { Index = Tracks.Count, Name = name ?? "" };
            Tracks.Add(t);
            return t;
        }

        public void AddAudioBus(string name = null)
        {
            AudioBuses.Add(name ?? "");
        }

        public Marker AddMarker(double seconds, string label = "Marcador")
        {
            var mk = new Marker(Timecode.FromSeconds(seconds), label);
            Markers.Add(mk);
            return mk;
        }
    }

    public sealed class Vegas
    {
        private readonly List<string> _messages = new List<string>();
        private readonly List<string> _commands = new List<string>();
        private readonly int _origBusCount;

        public Project Project { get; } = new Project();
        public TransportControl Transport { get; } = new TransportControl();

        internal Vegas(InputSnap snap)
        {
            Transport.CursorPosition = Timecode.FromMilliseconds(snap.cursor_ms);
            if (snap.tracks != null)
            {
                foreach (var t in snap.tracks)
                {
                    Track tr = t.is_video ? new VideoTrack() : new AudioTrack();
                    tr._origIndex = t.index;
                    tr.Index = t.index;
                    tr.Name = t.name ?? "";
                    tr.Mute = t.mute;
                    tr.Solo = t.solo;
                    Project.Tracks.Add(tr);
                }
            }
            if (snap.markers != null)
            {
                for (int i = 0; i < snap.markers.Count; i++)
                {
                    var m = snap.markers[i];
                    var mk = new Marker(Timecode.FromMilliseconds(m.position_ms), m.label)
                    {
                        _origIndex = i
                    };
                    Project.Markers.Add(mk);
                }
            }
            if (snap.audio_buses != null)
            {
                Project.AudioBuses.AddRange(snap.audio_buses);
            }
            _origBusCount = Project.AudioBuses.Count;
        }

        public VideoTrack AddVideoTrack(string name = null) => Project.AddVideoTrack(name);

        public AudioTrack AddAudioTrack(string name = null) => Project.AddAudioTrack(name);

        public void AddAudioBus(string name = null) => Project.AddAudioBus(name);

        public Marker AddMarker(double seconds, string label = "Marcador") => Project.AddMarker(seconds, label);

        public void RunCommand(string commandId)
        {
            if (!string.IsNullOrWhiteSpace(commandId))
            {
                _commands.Add(commandId.Trim());
            }
        }

        public void ShowMessage(string message)
        {
            if (message != null)
            {
                _messages.Add(message);
            }
        }

        // Devolve a lista FINAL de trilhas e marcadores: cada item diz de qual
        // item original veio (ou -1 se é novo) e só o que mudou. Assim
        // renomear, mover, apagar, reordenar e inserir no meio chegam ao
        // Neoveg em vez de se perderem em silêncio (AGENTS.md §0).
        internal OutputSnap BuildResult(InputSnap orig)
        {
            var tracksOut = new List<TrackOut>();
            var markersOut = new List<MarkerOut>();
            var addedBuses = new List<string>();

            var origTracks = orig.tracks ?? new List<TrackSnap>();
            var seenTracks = new HashSet<int>();
            foreach (var cur in Project.Tracks)
            {
                if (cur == null)
                {
                    continue;
                }
                if (cur._origIndex >= 0)
                {
                    if (!seenTracks.Add(cur._origIndex))
                    {
                        throw new InvalidOperationException(
                            "a mesma trilha aparece mais de uma vez em Project.Tracks");
                    }
                    TrackSnap old = null;
                    for (int j = 0; j < origTracks.Count; j++)
                    {
                        if (origTracks[j].index == cur._origIndex)
                        {
                            old = origTracks[j];
                            break;
                        }
                    }
                    if (old == null)
                    {
                        throw new InvalidOperationException("trilha original desconhecida");
                    }
                    tracksOut.Add(new TrackOut
                    {
                        orig = cur._origIndex,
                        is_video = cur.IsVideo(),
                        name = (cur.Name ?? "") != (old.name ?? "") ? (cur.Name ?? "") : null,
                        mute = cur.Mute != old.mute ? cur.Mute : (bool?)null,
                        solo = cur.Solo != old.solo ? cur.Solo : (bool?)null
                    });
                }
                else
                {
                    tracksOut.Add(new TrackOut
                    {
                        orig = -1,
                        is_video = cur.IsVideo(),
                        name = string.IsNullOrWhiteSpace(cur.Name) ? null : cur.Name,
                        mute = cur.Mute ? true : (bool?)null,
                        solo = cur.Solo ? true : (bool?)null
                    });
                }
            }

            var origMarkers = orig.markers ?? new List<MarkerSnap>();
            var seenMarkers = new HashSet<int>();
            foreach (var mk in Project.Markers)
            {
                if (mk == null)
                {
                    continue;
                }
                double ms = mk.Position != null ? mk.Position.Milliseconds : 0.0;
                string label = mk.Label ?? "";
                if (mk._origIndex >= 0 && mk._origIndex < origMarkers.Count)
                {
                    if (!seenMarkers.Add(mk._origIndex))
                    {
                        throw new InvalidOperationException(
                            "o mesmo marcador aparece mais de uma vez em Project.Markers");
                    }
                    var old = origMarkers[mk._origIndex];
                    markersOut.Add(new MarkerOut
                    {
                        orig = mk._origIndex,
                        seconds = ms != old.position_ms ? ms / 1000.0 : (double?)null,
                        label = label != (old.label ?? "") ? label : null
                    });
                }
                else
                {
                    markersOut.Add(new MarkerOut
                    {
                        orig = -1,
                        seconds = ms / 1000.0,
                        label = label
                    });
                }
            }

            for (int i = _origBusCount; i < Project.AudioBuses.Count; i++)
            {
                var bus = Project.AudioBuses[i];
                addedBuses.Add(string.IsNullOrWhiteSpace(bus) ? null : bus);
            }

            double? newCursor = Math.Abs(Transport.CursorPosition.Milliseconds - orig.cursor_ms) > 1e-6
                ? Transport.CursorPosition.Milliseconds
                : (double?)null;
            return new OutputSnap
            {
                ok = true,
                protocol = 2,
                messages = _messages,
                commands = _commands,
                tracks = tracksOut,
                added_buses = addedBuses,
                markers = markersOut,
                cursor_ms = newCursor
            };
        }
    }

    internal sealed class TrackSnap
    {
        public int index { get; set; }
        public string name { get; set; }
        public bool is_video { get; set; }
        public bool mute { get; set; }
        public bool solo { get; set; }
    }

    internal sealed class MarkerSnap
    {
        public double position_ms { get; set; }
        public string label { get; set; }
    }

    internal sealed class InputSnap
    {
        public string result_path { get; set; }
        public string target_assembly_path { get; set; }
        public double cursor_ms { get; set; }
        public List<TrackSnap> tracks { get; set; }
        public List<MarkerSnap> markers { get; set; }
        public List<string> audio_buses { get; set; }
    }

    internal sealed class TrackOut
    {
        public int orig { get; set; }
        public bool is_video { get; set; }
        public string name { get; set; }
        public bool? mute { get; set; }
        public bool? solo { get; set; }
    }

    internal sealed class MarkerOut
    {
        public int orig { get; set; }
        public double? seconds { get; set; }
        public string label { get; set; }
    }

    internal sealed class OutputSnap
    {
        public bool ok { get; set; }
        public string error { get; set; }
        public List<string> messages { get; set; } = new List<string>();
        public List<string> commands { get; set; } = new List<string>();
        public int protocol { get; set; }
        public List<TrackOut> tracks { get; set; } = new List<TrackOut>();
        public List<string> added_buses { get; set; } = new List<string>();
        public List<MarkerOut> markers { get; set; } = new List<MarkerOut>();
        public double? cursor_ms { get; set; }
    }

    internal sealed class NeovegScriptLoadContext : AssemblyLoadContext
    {
        private readonly string _baseDir;

        public NeovegScriptLoadContext(string baseDir) : base(isCollectible: true)
        {
            _baseDir = baseDir;
        }

        protected override Assembly Load(AssemblyName assemblyName)
        {
            if (string.Equals(assemblyName.Name, "ScriptPortal.Vegas", StringComparison.OrdinalIgnoreCase)
                || string.Equals(assemblyName.Name, "NeovegScript", StringComparison.OrdinalIgnoreCase))
            {
                return typeof(Vegas).Assembly;
            }
            if (!string.IsNullOrEmpty(_baseDir) && !string.IsNullOrEmpty(assemblyName.Name))
            {
                string cand = Path.Combine(_baseDir, assemblyName.Name + ".dll");
                if (File.Exists(cand))
                {
                    byte[] raw = File.ReadAllBytes(cand);
                    using var ms = new MemoryStream(raw, writable: false);
                    return LoadFromStream(ms);
                }
            }
            return null;
        }
    }

    public static class NeovegBridge
    {
        public static int RunEntry(IntPtr argsPtr, int sizeBytes)
        {
            if (argsPtr == IntPtr.Zero || sizeBytes <= 0)
            {
                return 2;
            }
            string resultPath = null;
            NeovegScriptLoadContext loadCtx = null;
            try
            {
                string json = Marshal.PtrToStringUTF8(argsPtr, sizeBytes);
                var snap = JsonSerializer.Deserialize<InputSnap>(json);
                if (snap == null || string.IsNullOrWhiteSpace(snap.result_path))
                {
                    return 2;
                }
                resultPath = snap.result_path;
                Assembly targetAsm = typeof(NeovegBridge).Assembly;
                if (!string.IsNullOrWhiteSpace(snap.target_assembly_path))
                {
                    string fullTarget = Path.GetFullPath(snap.target_assembly_path);
                    loadCtx = new NeovegScriptLoadContext(Path.GetDirectoryName(fullTarget));
                    byte[] raw = File.ReadAllBytes(fullTarget);
                    using var ms = new MemoryStream(raw, writable: false);
                    targetAsm = loadCtx.LoadFromStream(ms);

                    var foreignBridge = targetAsm.GetType("ScriptPortal.Vegas.NeovegBridge", throwOnError: false);
                    if (foreignBridge != null && foreignBridge != typeof(NeovegBridge))
                    {
                        var foreignEntry = foreignBridge.GetMethod(
                            "RunEntry",
                            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static,
                            null,
                            new[] { typeof(IntPtr), typeof(int) },
                            null
                        );
                        if (foreignEntry != null)
                        {
                            snap.target_assembly_path = null;
                            string fwdJson = JsonSerializer.Serialize(snap);
                            int fwdBytes = Encoding.UTF8.GetByteCount(fwdJson);
                            IntPtr fwdPtr = Marshal.StringToCoTaskMemUTF8(fwdJson);
                            try
                            {
                                object rc = foreignEntry.Invoke(null, new object[] { fwdPtr, fwdBytes });
                                return rc is int code ? code : 0;
                            }
                            finally
                            {
                                Marshal.ZeroFreeCoTaskMemUTF8(fwdPtr);
                            }
                        }
                    }
                }

                var vegas = new Vegas(snap);
                bool invoked = InvokeUserEntryPoint(targetAsm, vegas);
                if (!invoked)
                {
                    var err = new OutputSnap
                    {
                        ok = false,
                        error = "método EntryPoint.FromVegas(Vegas) ou Main() não encontrado no script C#/.NET"
                    };
                    File.WriteAllText(resultPath, JsonSerializer.Serialize(err));
                    return 1;
                }
                var outSnap = vegas.BuildResult(snap);
                File.WriteAllText(resultPath, JsonSerializer.Serialize(outSnap));
                return 0;
            }
            catch (Exception ex)
            {
                var inner = ex is TargetInvocationException tie && tie.InnerException != null ? tie.InnerException : ex;
                if (!string.IsNullOrWhiteSpace(resultPath))
                {
                    try
                    {
                        var err = new OutputSnap
                        {
                            ok = false,
                            error = inner.GetType().Name + ": " + inner.Message
                        };
                        File.WriteAllText(resultPath, JsonSerializer.Serialize(err));
                    }
                    catch { }
                }
                return 1;
            }
            finally
            {
                if (loadCtx != null)
                {
                    try { loadCtx.Unload(); } catch { }
                }
            }
        }

        private static bool InvokeUserEntryPoint(Assembly asm, Vegas vegas)
        {
            Type[] types;
            try
            {
                types = asm.GetTypes();
            }
            catch (ReflectionTypeLoadException rtle)
            {
                types = rtle.Types;
            }
            const BindingFlags flags =
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;
            bool sawForeignVegasParam = false;

            foreach (var passType in new[] { typeof(Vegas), typeof(object) })
            {
                foreach (var ty in types)
                {
                    if (ty == null || ty.Namespace == "ScriptPortal.Vegas")
                    {
                        continue;
                    }
                    var mi = ty.GetMethod("FromVegas", flags, null, new[] { passType }, null);
                    if (mi != null && (mi.IsStatic || !ty.IsAbstract))
                    {
                        object target = mi.IsStatic ? null : Activator.CreateInstance(ty, nonPublic: true);
                        mi.Invoke(target, new object[] { vegas });
                        return true;
                    }
                    if (passType == typeof(Vegas))
                    {
                        foreach (var cand in ty.GetMethods(flags))
                        {
                            if (cand.Name != "FromVegas")
                            {
                                continue;
                            }
                            var ps = cand.GetParameters();
                            if (ps.Length == 1 && ps[0].ParameterType.FullName == "ScriptPortal.Vegas.Vegas")
                            {
                                sawForeignVegasParam = true;
                            }
                        }
                    }
                }
            }

            if (sawForeignVegasParam)
            {
                throw new InvalidOperationException(
                    "parâmetro 'ScriptPortal.Vegas.Vegas' incompatível em assembly externo; compile referenciando NeovegScript ou use 'FromVegas(dynamic vegas)'"
                );
            }

            foreach (var ty in types)
            {
                if (ty == null || ty.Namespace == "ScriptPortal.Vegas")
                {
                    continue;
                }
                foreach (var mainName in new[] { "Main", "<Main>$" })
                {
                    var mainNoArg = mainName == "Main"
                        ? ty.GetMethod(mainName, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static, null, Type.EmptyTypes, null)
                        : null;
                    if (mainNoArg != null)
                    {
                        mainNoArg.Invoke(null, null);
                        return true;
                    }
                    var mainArgs = ty.GetMethod(
                        mainName,
                        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static,
                        null,
                        new[] { typeof(string[]) },
                        null
                    );
                    if (mainArgs != null)
                    {
                        mainArgs.Invoke(null, new object[] { Array.Empty<string>() });
                        return true;
                    }
                }
            }
            return false;
        }
    }
}
"#;

fn build_snapshot(
    session: &EditorSession,
    result_path: &Path,
    target_assembly: Option<&Path>,
) -> Result<Vec<u8>, NeovegError> {
    let tracks = session
        .project
        .tracks
        .iter()
        .enumerate()
        .map(|(index, t)| ScriptTrackSnap {
            index,
            name: t.name.clone(),
            is_video: t.kind == TrackKind::Video,
            mute: t.mute,
            solo: t.solo,
        })
        .collect();
    let markers = session
        .project
        .markers
        .iter()
        .map(|m| ScriptMarkerSnap {
            position_ms: (m.t.0 as f64) / 1_411_200.0,
            label: m.label.clone(),
        })
        .collect();
    let audio_buses = session
        .project
        .audio_buses
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let payload = ScriptInputPayload {
        result_path: result_path.to_string_lossy().to_string(),
        target_assembly_path: target_assembly.map(|p| p.to_string_lossy().to_string()),
        cursor_ms: (session.cursor.0 as f64) / 1_411_200.0,
        tracks,
        markers,
        audio_buses,
    };
    serde_json::to_vec(&payload)
        .map_err(|e| NeovegError::Json(format!("erro ao serializar contexto do script: {e}")))
}

fn validate_managed_dll_header(path: &Path) -> Result<(), NeovegError> {
    let mut file = std::fs::File::open(path).map_err(|e| {
        NeovegError::Io(format!("erro ao abrir assembly '{}': {e}", path.display()))
    })?;
    let meta = file.metadata().map_err(|e| {
        NeovegError::Io(format!(
            "erro ao inspecionar assembly '{}': {e}",
            path.display()
        ))
    })?;
    let file_len = meta.len();
    if file_len < 128 {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo '{}' é pequeno demais para ser um assembly .NET (.dll) válido",
            path.display()
        )));
    }
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).map_err(|e| {
        NeovegError::InvalidInput(format!(
            "erro ao ler cabeçalho DOS de '{}': {e}",
            path.display()
        ))
    })?;
    if dos.get(0..2) != Some(b"MZ".as_slice()) {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo '{}' não possui assinatura PE ('MZ') válida",
            path.display()
        )));
    }
    let e_lfanew = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    if !(64..=1024 * 1024).contains(&e_lfanew)
        || e_lfanew.checked_add(120).is_none_or(|end| end > file_len)
    {
        return Err(NeovegError::InvalidInput(format!(
            "cabeçalho PE de '{}' possui deslocamento inválido",
            path.display()
        )));
    }
    file.seek(SeekFrom::Start(e_lfanew)).map_err(|e| {
        NeovegError::InvalidInput(format!(
            "falha ao posicionar cabeçalho PE de '{}': {e}",
            path.display()
        ))
    })?;
    let mut pe = [0u8; 264];
    let mut read_total = 0usize;
    while read_total < pe.len() {
        let Some(slice) = pe.get_mut(read_total..) else {
            break;
        };
        match file.read(slice) {
            Ok(0) => break,
            Ok(n) => read_total += n,
            Err(e) => {
                return Err(NeovegError::InvalidInput(format!(
                    "falha ao ler cabeçalho PE de '{}': {e}",
                    path.display()
                )));
            }
        }
    }
    let Some(head) = pe.get(..read_total) else {
        return Err(NeovegError::InvalidInput(
            "cabeçalho PE truncado em arquivo .dll".into(),
        ));
    };
    if head.get(0..4) != Some(b"PE\0\0".as_slice()) {
        return Err(NeovegError::InvalidInput(format!(
            "assinatura 'PE\\0\\0' ausente em '{}'",
            path.display()
        )));
    }
    let (Some(opt_sz_b), Some(magic_b)) = (head.get(20..22), head.get(24..26)) else {
        return Err(NeovegError::InvalidInput(format!(
            "cabeçalho COFF/Opcional truncado em '{}'",
            path.display()
        )));
    };
    let opt_size = u16::from_le_bytes([opt_sz_b[0], opt_sz_b[1]]) as usize;
    let magic = u16::from_le_bytes([magic_b[0], magic_b[1]]);
    let (rva_cnt_off, clr_off) = match magic {
        0x010b => (24 + 92, 24 + 208),
        0x020b => (24 + 108, 24 + 224),
        _ => {
            return Err(NeovegError::InvalidInput(format!(
                "formato PE opcional (0x{magic:04x}) inválido em '{}'",
                path.display()
            )));
        }
    };
    if opt_size < (clr_off - 24) + 8 || head.len() < clr_off + 8 {
        return Err(NeovegError::InvalidInput(format!(
            "diretório CLR ausente no cabeçalho opcional de '{}'",
            path.display()
        )));
    }
    let Some(cnt_b) = head.get(rva_cnt_off..rva_cnt_off + 4) else {
        return Err(NeovegError::InvalidInput(
            "cabeçalho PE truncado antes de NumberOfRvaAndSizes".into(),
        ));
    };
    let num_dirs = u32::from_le_bytes([cnt_b[0], cnt_b[1], cnt_b[2], cnt_b[3]]);
    let Some(clr_b) = head.get(clr_off..clr_off + 8) else {
        return Err(NeovegError::InvalidInput(
            "cabeçalho PE truncado antes do diretório CLR".into(),
        ));
    };
    let clr_rva = u32::from_le_bytes([clr_b[0], clr_b[1], clr_b[2], clr_b[3]]);
    let clr_size = u32::from_le_bytes([clr_b[4], clr_b[5], clr_b[6], clr_b[7]]);
    if num_dirs < 15 || clr_rva == 0 || clr_size < 72 {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo '{}' não é um assembly .NET gerenciado válido (metadados CLR ausentes)",
            path.display()
        )));
    }
    Ok(())
}

fn compile_cs_project(
    env: &DotnetEnv,
    source_or_none: Option<&str>,
    stage_dir: &Path,
) -> Result<PathBuf, NeovegError> {
    if !env.has_sdk {
        return Err(NeovegError::Unsupported(
            "runtime .NET (hostfxr) encontrado, mas o .NET SDK é necessário para compilar scripts '.cs'; use '--command <id>' ou um assembly compilado (.dll)".into(),
        ));
    }
    let cli = env
        .dotnet_cli
        .as_deref()
        .unwrap_or_else(|| Path::new("dotnet"));
    let src_dir = stage_dir.join("src");
    let out_dir = stage_dir.join("out");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| NeovegError::Io(format!("erro ao criar pasta de script: {e}")))?;
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| NeovegError::Io(format!("erro ao criar pasta de saída do script: {e}")))?;

    std::fs::write(
        src_dir.join("NeovegScript.csproj"),
        cs_project_xml(&env.tfm),
    )
    .map_err(|e| NeovegError::Io(format!("erro ao gravar .csproj: {e}")))?;
    std::fs::write(src_dir.join("ScriptPortalVegas.cs"), SCRIPT_PORTAL_VEGAS_CS)
        .map_err(|e| NeovegError::Io(format!("erro ao gravar ScriptPortalVegas.cs: {e}")))?;
    if let Some(source) = source_or_none {
        std::fs::write(src_dir.join("UserScript.cs"), source)
            .map_err(|e| NeovegError::Io(format!("erro ao preparar UserScript.cs: {e}")))?;
    }

    let output = Command::new(cli)
        .arg("build")
        .arg(src_dir.join("NeovegScript.csproj"))
        .arg("-c")
        .arg("Release")
        .arg("--nologo")
        .arg("-v")
        .arg("minimal")
        .arg("-nodeReuse:false")
        .arg("-p:UseSharedCompilation=false")
        .arg("-o")
        .arg(&out_dir)
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1")
        .output()
        .map_err(|e| NeovegError::Io(format!("falha ao invocar 'dotnet build': {e}")))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let diag = truncate_str(
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            },
            MAX_DIAGNOSTIC_CHARS,
        );
        return Err(NeovegError::InvalidInput(format!(
            "erro ao compilar script C#: {diag}"
        )));
    }

    let dll_path = out_dir.join("NeovegScript.dll");
    if !dll_path.is_file() {
        return Err(NeovegError::Internal(
            "compilação .NET concluiu sem gerar NeovegScript.dll".into(),
        ));
    }
    Ok(dll_path)
}

fn ensure_bridge_dll(env: &DotnetEnv) -> Result<PathBuf, NeovegError> {
    let cached = BRIDGE_DLL.get_or_init(|| {
        let dir = std::env::temp_dir()
            .join(format!("neoveg_hostfxr_{}", std::process::id()))
            .join("bridge");
        compile_cs_project(env, None, &dir).map_err(|e| e.to_string())
    });
    match cached {
        Ok(p) => Ok(p.clone()),
        Err(msg) => Err(NeovegError::Internal(msg.clone())),
    }
}

/// A lista final é a original, na mesma ordem e sem mudanças?
fn tracks_unchanged(out: &[ScriptTrackOut], n: usize) -> bool {
    out.len() == n
        && out.iter().enumerate().all(|(i, t)| {
            usize::try_from(t.orig).ok() == Some(i)
                && t.name.is_none()
                && t.mute.is_none()
                && t.solo.is_none()
        })
}

fn markers_unchanged(out: &[ScriptMarkerOut], n: usize) -> bool {
    out.len() == n
        && out.iter().enumerate().all(|(i, m)| {
            usize::try_from(m.orig).ok() == Some(i) && m.seconds.is_none() && m.label.is_none()
        })
}

/// Índice original válido e ainda não usado (§3.1: cada identidade uma vez).
fn take_orig(orig: i64, used: &mut [bool], what: &str) -> Result<Option<usize>, NeovegError> {
    let Ok(i) = usize::try_from(orig) else {
        return Ok(None);
    };
    match used.get_mut(i) {
        None => Err(NeovegError::InvalidInput(format!(
            "script .NET referenciou {what} original inexistente: {i}"
        ))),
        Some(true) => Err(NeovegError::InvalidInput(format!(
            "script .NET repetiu {what} {i} na lista final"
        ))),
        Some(u) => {
            *u = true;
            Ok(Some(i))
        }
    }
}

/// Remonta as trilhas na ordem final do script. Trilhas que ficaram de fora
/// foram apagadas pelo script; trilhas existentes mantêm id, eventos e tudo
/// o que o script não mudou.
fn rebuild_tracks(old: &[Track], out: Vec<ScriptTrackOut>) -> Result<Vec<Track>, NeovegError> {
    let mut used = vec![false; old.len()];
    let mut tracks: Vec<Track> = Vec::with_capacity(out.len());
    for t in out {
        let mut tr = match take_orig(t.orig, &mut used, "trilha")? {
            Some(i) => {
                let Some(src) = old.get(i) else {
                    return Err(NeovegError::Internal(
                        "índice de trilha fora da lista".into(),
                    ));
                };
                let mut tr = src.clone();
                if let Some(name) = t.name {
                    tr.name = truncate_str(name.trim(), 128);
                }
                tr
            }
            None => {
                let kind = if t.is_video {
                    TrackKind::Video
                } else {
                    TrackKind::Audio
                };
                let n = tracks.iter().filter(|x| x.kind == kind).count() + 1;
                let name = t
                    .name
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| match kind {
                        TrackKind::Video => format!("Vídeo {n}"),
                        TrackKind::Audio => format!("Áudio {n}"),
                    });
                Track::new(kind, &truncate_str(&name, 128))
            }
        };
        if let Some(m) = t.mute {
            tr.mute = m;
        }
        if let Some(s) = t.solo {
            tr.solo = s;
        }
        tracks.push(tr);
    }
    Ok(tracks)
}

fn marker_ticks(seconds: f64) -> Result<Ticks, NeovegError> {
    if !seconds.is_finite() {
        return Err(NeovegError::InvalidInput(
            "script .NET informou tempo não finito para marcador".into(),
        ));
    }
    let secs = seconds.clamp(0.0, 48.0 * 3600.0);
    let ticks_i = (secs * 1_411_200_000.0).round() as i64;
    Ok(Ticks::from_raw(ticks_i).clamp_safe(Ticks::ZERO, Ticks::MAX))
}

fn marker_label(label: &str) -> String {
    if label.trim().is_empty() {
        "Marcador".into()
    } else {
        truncate_str(label.trim(), 128)
    }
}

/// Remonta os marcadores na ordem final do script (mesmas regras das
/// trilhas). Um marcador que não mudou mantém id e tempo exatos.
fn rebuild_markers(old: &[Marker], out: Vec<ScriptMarkerOut>) -> Result<Vec<Marker>, NeovegError> {
    let mut used = vec![false; old.len()];
    let mut markers: Vec<Marker> = Vec::with_capacity(out.len());
    for m in out {
        match take_orig(m.orig, &mut used, "marcador")? {
            Some(i) => {
                let Some(src) = old.get(i) else {
                    return Err(NeovegError::Internal(
                        "índice de marcador fora da lista".into(),
                    ));
                };
                let mut mk = src.clone();
                if let Some(s) = m.seconds {
                    mk.t = marker_ticks(s)?;
                }
                if let Some(l) = m.label {
                    mk.label = marker_label(&l);
                }
                markers.push(mk);
            }
            None => markers.push(Marker {
                id: neoveg_engine::model::new_id(),
                t: marker_ticks(m.seconds.unwrap_or(0.0))?,
                label: marker_label(m.label.as_deref().unwrap_or("")),
            }),
        }
    }
    Ok(markers)
}

fn apply_script_output(
    out: ScriptOutputPayload,
    session: &mut EditorSession,
    host: &mut HeadlessHost,
    messages_out: &mut Vec<String>,
) -> Result<(), NeovegError> {
    if !out.ok {
        let msg = out
            .error
            .unwrap_or_else(|| "exceção desconhecida ao executar script C#".into());
        return Err(NeovegError::InvalidInput(format!(
            "erro em tempo de execução no script C#: {msg}"
        )));
    }

    if out.protocol != SCRIPT_OUTPUT_PROTOCOL {
        return Err(NeovegError::Unsupported(
            "o assembly '.dll' foi gerado com uma versão antiga da ponte ScriptPortal.Vegas \
             do Neoveg; recompile o script"
                .into(),
        ));
    }
    let n_tracks = session.project.tracks.len();
    let n_markers = session.project.markers.len();
    if out.tracks.len() > n_tracks.saturating_add(MAX_SCRIPT_ITEMS)
        || out.added_buses.len() > MAX_SCRIPT_ITEMS
        || out.markers.len() > n_markers.saturating_add(MAX_SCRIPT_ITEMS)
        || out.commands.len() > MAX_SCRIPT_ITEMS
        || out.messages.len() > MAX_SCRIPT_ITEMS
    {
        return Err(NeovegError::InvalidInput(format!(
            "saída do script excede o limite de {MAX_SCRIPT_ITEMS} operações por execução"
        )));
    }

    let tracks_changed = !tracks_unchanged(&out.tracks, n_tracks);
    let markers_changed = !markers_unchanged(&out.markers, n_markers);
    let has_project_mutations = tracks_changed || !out.added_buses.is_empty() || markers_changed;
    let had_history_cmd = out
        .commands
        .iter()
        .any(|c| matches!(c.as_str(), "edit.undo" | "edit.redo"));

    let orig_project = session.project.clone();
    let mut staged = session.clone();
    let saved_undo = if !had_history_cmd {
        std::mem::take(&mut staged.undo)
    } else {
        if has_project_mutations {
            staged.checkpoint();
        }
        Vec::new()
    };

    if tracks_changed {
        staged.project.tracks = rebuild_tracks(&staged.project.tracks, out.tracks)?;
    }

    for bus_name in out.added_buses {
        let clean = bus_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if staged.project.add_audio_bus(clean).is_none() {
            return Err(NeovegError::InvalidInput(
                "script .NET excedeu o limite máximo de 26 barramentos auxiliares de áudio".into(),
            ));
        }
    }

    if markers_changed {
        staged.project.markers = rebuild_markers(&staged.project.markers, out.markers)?;
    }

    let cursor_changed = if let Some(ms) = out.cursor_ms {
        if !ms.is_finite() {
            return Err(NeovegError::InvalidInput(
                "script .NET informou posição de cursor não finita".into(),
            ));
        }
        let clamped = ms.clamp(0.0, (48.0 * 3600.0) * 1000.0);
        staged.cursor = Ticks::from_raw((clamped * 1_411_200.0).round() as i64)
            .clamp_safe(Ticks::ZERO, Ticks::MAX);
        true
    } else {
        false
    };

    for cmd_id in out.commands {
        let host_trait: &mut dyn CommandHost = host;
        if let Err(e) = run_command(&cmd_id, &mut staged, host_trait) {
            return Err(NeovegError::InvalidInput(format!(
                "script C# falhou ao executar comando '{cmd_id}': {e}"
            )));
        }
    }

    let cmd_checkpointed = if !had_history_cmd {
        let pushed = !staged.undo.is_empty();
        staged.undo = saved_undo;
        if has_project_mutations || pushed {
            staged.undo.push(orig_project);
            if staged.undo.len() > MAX_UNDO {
                staged.undo.remove(0);
            }
            staged.redo.clear();
        }
        pushed
    } else {
        false
    };

    if has_project_mutations || cmd_checkpointed {
        let _ = staged.project.sanitize();
        staged.changed();
    } else if cursor_changed {
        staged.generation += 1;
    }

    *session = staged;
    messages_out.extend(out.messages);
    Ok(())
}

/// Compila (para `.cs`) e executa um script C# ou assembly `.dll` usando o
/// runtime `.NET Core` carregado sob demanda por `netcorehost`.
pub fn run_cs_script_impl(
    script_path: &Path,
    source_or_none: Option<&str>,
    session: &mut EditorSession,
    host: &mut HeadlessHost,
    messages_out: &mut Vec<String>,
) -> Result<(), NeovegError> {
    if source_or_none.is_none() {
        validate_managed_dll_header(script_path)?;
    }

    let env = dotnet_host::find_dotnet_env()?;
    let seq = RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let stage = TempDirGuard(std::env::temp_dir().join(format!(
        "neoveg_cs_{}_{}",
        std::process::id(),
        seq
    )));
    std::fs::create_dir_all(&stage.0)
        .map_err(|e| NeovegError::Io(format!("erro ao criar pasta temporária .NET: {e}")))?;

    let result_path = stage.0.join("script_result.json");
    let mut last_exit_code = 0;

    if let Some(src) = source_or_none {
        let dll_path = compile_cs_project(&env, Some(src), &stage.0)?;
        let payload = build_snapshot(session, &result_path, None)?;
        last_exit_code = dotnet_host::invoke_component(
            &env,
            &dll_path,
            "ScriptPortal.Vegas.NeovegBridge, NeovegScript",
            "RunEntry",
            &payload,
        )?;
    } else if env.has_sdk {
        let loader_dll = ensure_bridge_dll(&env)?;
        let bridge_payload = build_snapshot(session, &result_path, Some(script_path))?;
        last_exit_code = dotnet_host::invoke_component(
            &env,
            &loader_dll,
            "ScriptPortal.Vegas.NeovegBridge, NeovegScript",
            "RunEntry",
            &bridge_payload,
        )?;
    } else {
        // Sem SDK: faz cópia temporária para não bloquear o `.dll` original em disco (Windows)
        // e invoca diretamente `ScriptPortal.Vegas.NeovegBridge` caso o assembly a exporte.
        let file_name = script_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("script.dll"));
        let shadow_dll = stage.0.join(file_name);
        std::fs::copy(script_path, &shadow_dll).map_err(|e| {
            NeovegError::Io(format!(
                "erro ao preparar cópia temporária de '{}': {e}",
                script_path.display()
            ))
        })?;

        let payload = build_snapshot(session, &result_path, None)?;
        let mut type_candidates = Vec::new();
        if let Some(stem) = script_path.file_stem().and_then(|s| s.to_str()) {
            let trimmed = stem.trim();
            if !trimmed.is_empty() && !trimmed.contains(',') {
                type_candidates.push(format!("ScriptPortal.Vegas.NeovegBridge, {trimmed}"));
            }
        }
        let neoveg_default = "ScriptPortal.Vegas.NeovegBridge, NeovegScript".to_string();
        if !type_candidates.contains(&neoveg_default) {
            type_candidates.push(neoveg_default);
        }

        let mut direct_err: Option<NeovegError> = None;
        for cand_type in &type_candidates {
            match dotnet_host::invoke_component(&env, &shadow_dll, cand_type, "RunEntry", &payload)
            {
                Ok(code) => {
                    last_exit_code = code;
                    break;
                }
                Err(e) => {
                    if result_path.is_file() {
                        break;
                    }
                    direct_err = Some(e);
                }
            }
        }

        if !result_path.is_file() {
            let detail = direct_err.map(|e| format!(" ({e})")).unwrap_or_default();
            return Err(NeovegError::Unsupported(format!(
                "assembly '.dll' não expõe ponto de entrada direto 'ScriptPortal.Vegas.NeovegBridge' e o .NET SDK não está disponível para hospedar assemblies externos{detail}"
            )));
        }
    }

    if !result_path.is_file() {
        return Err(NeovegError::InvalidInput(format!(
            "script .NET retornou código {last_exit_code} sem produzir resposta de bridge ScriptPortal.Vegas"
        )));
    }
    let meta = std::fs::metadata(&result_path)
        .map_err(|e| NeovegError::Io(format!("erro ao inspecionar saída do script: {e}")))?;
    if meta.len() > MAX_SCRIPT_OUTPUT_BYTES {
        return Err(NeovegError::InvalidInput(
            "saída do script C# excede o limite de 4 MiB".into(),
        ));
    }
    let res_text = std::fs::read_to_string(&result_path)
        .map_err(|e| NeovegError::Io(format!("erro ao ler saída do script C#: {e}")))?;
    let parsed: ScriptOutputPayload = serde_json::from_str(&res_text)
        .map_err(|e| NeovegError::Json(format!("resposta JSON inválida do script C#: {e}")))?;

    apply_script_output(parsed, session, host, messages_out)
}
