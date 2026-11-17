//! Ponte gerenciada `ScriptPortal.Vegas` (Sala Limpa) e aplicador de mutações
//! no `EditorSession` sem uso de `unsafe`.

use crate::dotnet_host::{self, DotnetEnv};
use neoveg_common::error::NeovegError;
use neoveg_engine::commands::{run_command, CommandHost, HeadlessHost};
use neoveg_engine::media;
use neoveg_engine::model::{Marker, MediaItem, MediaSource, Take, Track, TrackKind};
use neoveg_engine::session::{EditorSession, MAX_UNDO};
use neoveg_engine::takes;
use neoveg_engine::time::Ticks;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::sync::{Mutex, MutexGuard, PoisonError};

const MAX_SCRIPT_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIAGNOSTIC_CHARS: usize = 1200;
const MAX_SCRIPT_ITEMS: usize = 4096;
static RUN_SEQ: AtomicU64 = AtomicU64::new(1);
static BRIDGE_DLL: OnceLock<Result<PathBuf, String>> = OnceLock::new();

#[derive(Serialize)]
struct ScriptEventSnap {
    id: u64,
    name: String,
    start: i64,
    length: i64,
    mute: bool,
    locked: bool,
    fade_in: i64,
    fade_out: i64,
    takes: Vec<ScriptTakeSnap>,
    active_take: usize,
}

#[derive(Serialize)]
struct ScriptTakeSnap {
    media_id: u64,
    offset: i64,
    name: String,
}

/// Mídia do projeto como o script a vê (`Media`, `MediaPool`).
#[derive(Serialize)]
struct ScriptMediaSnap {
    id: u64,
    path: String,
    name: String,
    has_video: bool,
    has_audio: bool,
    generated: bool,
    /// ticks; 0 = sem duração (imagem estática, mídia gerada)
    length: i64,
    width: u32,
    height: u32,
    fps: f64,
}

#[derive(Serialize)]
struct ScriptTrackSnap {
    index: usize,
    name: String,
    is_video: bool,
    mute: bool,
    solo: bool,
    events: Vec<ScriptEventSnap>,
}

#[derive(Serialize)]
struct ScriptMarkerSnap {
    position_ticks: i64,
    label: String,
}

#[derive(Serialize)]
struct ScriptInputPayload {
    result_path: String,
    target_assembly_path: Option<String>,
    fps_num: i64,
    fps_den: i64,
    cursor_ticks: i64,
    sel_start_ticks: i64,
    sel_length_ticks: i64,
    selected_track: i64,
    selected_events: Vec<u64>,
    tracks: Vec<ScriptTrackSnap>,
    markers: Vec<ScriptMarkerSnap>,
    audio_buses: Vec<String>,
    media: Vec<ScriptMediaSnap>,
    /// Endereço da função que importa mídia durante o script
    /// (`dotnet_host::media_probe_fn_addr`) e a execução a que pertence.
    probe_fn: u64,
    probe_token: u64,
}

/// Versão do formato de saída da ponte. Uma saída de outra versão veio de
/// uma ponte antiga embutida numa `.dll`: é recusada, nunca reinterpretada.
const SCRIPT_OUTPUT_PROTOCOL: u32 = 4;

/// Take na lista final de um evento.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptTakeOut {
    media_id: u64,
    offset: i64,
    name: String,
}

/// Evento na lista final de uma trilha: `id` do evento original e só o que
/// mudou (`None` = igual), ou `is_new` para um evento criado pelo script
/// (vem inteiro). Tempos em ticks do Neoveg.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptEventOut {
    id: u64,
    is_new: bool,
    selected: bool,
    takes: Option<Vec<ScriptTakeOut>>,
    active_take: Option<i64>,
    name: Option<String>,
    start: Option<i64>,
    length: Option<i64>,
    mute: Option<bool>,
    locked: Option<bool>,
    fade_in: Option<i64>,
    fade_out: Option<i64>,
}

impl ScriptEventOut {
    fn unchanged(&self) -> bool {
        !self.is_new
            && self.takes.is_none()
            && self.active_take.is_none()
            && self.name.is_none()
            && self.start.is_none()
            && self.length.is_none()
            && self.mute.is_none()
            && self.locked.is_none()
            && self.fade_in.is_none()
            && self.fade_out.is_none()
    }
}

/// Trilha na lista final devolvida pelo script: `orig` é o índice original
/// (ou negativo se é nova); `None` = não mudou; `events` é a lista final de
/// eventos da trilha.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptTrackOut {
    orig: i64,
    is_video: bool,
    name: Option<String>,
    mute: Option<bool>,
    solo: Option<bool>,
    selected: bool,
    events: Vec<ScriptEventOut>,
}

/// Marcador na lista final devolvida pelo script (mesma convenção).
#[derive(Deserialize, Default)]
#[serde(default)]
struct ScriptMarkerOut {
    orig: i64,
    ticks: Option<i64>,
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
    selected_events: Vec<u64>,
    cursor_ticks: Option<i64>,
    sel_start_ticks: Option<i64>,
    sel_length_ticks: Option<i64>,
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

/// Projeto dos scripts. `ImplicitUsings` fica desligado, como no .NET
/// Framework dos scripts do VEGAS: com ele, `System.Threading` entra sozinho e
/// um script com `using System.Windows.Forms;` e `Timer` não compila (CS0104,
/// `System.Windows.Forms.Timer` × `System.Threading.Timer`). Com o WinForms
/// ele também importaria `System.Windows.Forms` sem o script pedir, o que só
/// funciona no Windows.
fn cs_project_xml(tfm: &str, winforms: bool) -> String {
    // System.Windows.Forms real no Windows (com o runtime WindowsDesktop)
    let winforms = if winforms {
        "\n    <UseWindowsForms>true</UseWindowsForms>"
    } else {
        ""
    };
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{tfm}</TargetFramework>
    <OutputType>Library</OutputType>
    <AssemblyName>NeovegScript</AssemblyName>
    <ImplicitUsings>disable</ImplicitUsings>
    <Nullable>disable</Nullable>
    <ProduceReferenceAssembly>false</ProduceReferenceAssembly>
    <UseSharedCompilation>false</UseSharedCompilation>{winforms}
  </PropertyGroup>
</Project>
"#
    )
}

const SCRIPT_PORTAL_VEGAS_CS: &str = r#"using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Numerics;
using System.Reflection;
using System.Runtime.ExceptionServices;
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

    // Nomes e assinaturas desta ponte seguem a documentação pública da
    // MAGIX: "VEGAS Pro Scripting API Summary" (revisão de 28/08/2024,
    // help.magix-hub.com/video/vegas/sdk) e o FAQ de scripting do manual do
    // VEGAS Pro 21 (p. 1038). A implementação é própria (sala limpa).

    // Timecode (Scripting API Summary: Timecode). Guarda ticks inteiros do
    // Neoveg (1_411_200_000 por segundo) para a aritmética de edição ser
    // exata (AGENTS.md, Regras Rust 5); Nanos (unidades de 100 ns) é uma
    // conversão arredondada.
    public sealed class Timecode : IComparable
    {
        internal const long TicksPerSecond = 1_411_200_000L;
        internal static long FpsNum = 30;
        internal static long FpsDen = 1;
        internal long _ticks;

        public Timecode() { }

        [Obsolete("Use Timecode.FromMilliseconds")]
        public Timecode(double milliseconds) { _ticks = MsToTicks(milliseconds); }

        internal static Timecode FromTicks(long ticks) => new Timecode { _ticks = ticks };

        internal static long MsToTicks(double ms)
        {
            if (!double.IsFinite(ms))
            {
                throw new ArgumentException("tempo não finito");
            }
            double t = Math.Round(ms * (TicksPerSecond / 1000.0));
            if (t > long.MaxValue / 2 || t < long.MinValue / 2)
            {
                throw new OverflowException("tempo fora do limite");
            }
            return (long)t;
        }

        public static Timecode FromMilliseconds(double milliseconds) => FromTicks(MsToTicks(milliseconds));
        public static Timecode FromSeconds(double seconds) => FromTicks(MsToTicks(seconds * 1000.0));
        public static Timecode FromNanos(long nanos) => FromTicks((long)((BigInteger)nanos * 14112 / 100));

        public static Timecode FromFrames(long frames)
        {
            BigInteger t = (BigInteger)frames * TicksPerSecond * FpsDen / FpsNum;
            return FromTicks((long)t);
        }

        public double Milliseconds
        {
            get => _ticks / (TicksPerSecond / 1000.0);
            set => _ticks = MsToTicks(value);
        }

        public long Nanos
        {
            get => (long)(((BigInteger)_ticks * 100 + 7056) / 14112);
            set => _ticks = FromNanos(value)._ticks;
        }

        public long FrameCount
        {
            get => (long)((BigInteger)_ticks * FpsNum / ((BigInteger)TicksPerSecond * FpsDen));
            set => _ticks = FromFrames(value)._ticks;
        }

        public double FrameRate => (double)FpsNum / FpsDen;
        public double ToMilliseconds() => Milliseconds;

        public int CompareTo(object obj) => obj is Timecode o ? _ticks.CompareTo(o._ticks) : 1;
        public override bool Equals(object obj) => obj is Timecode o && o._ticks == _ticks;
        public override int GetHashCode() => _ticks.GetHashCode();

        public override string ToString()
        {
            long ms = (long)Math.Round(Milliseconds);
            var sign = ms < 0 ? "-" : "";
            ms = Math.Abs(ms);
            return string.Format("{0}{1:00}:{2:00}:{3:00}.{4:000}", sign, ms / 3600000, ms / 60000 % 60, ms / 1000 % 60, ms % 1000);
        }

        private static long T(Timecode t) => t is null ? throw new ArgumentNullException(nameof(t)) : t._ticks;
        public static bool operator ==(Timecode a, Timecode b) => a is null ? b is null : b is not null && a._ticks == b._ticks;
        public static bool operator !=(Timecode a, Timecode b) => !(a == b);
        public static bool operator <(Timecode a, Timecode b) => T(a) < T(b);
        public static bool operator >(Timecode a, Timecode b) => T(a) > T(b);
        public static bool operator <=(Timecode a, Timecode b) => T(a) <= T(b);
        public static bool operator >=(Timecode a, Timecode b) => T(a) >= T(b);
        public static Timecode operator +(Timecode a, Timecode b) => FromTicks(checked(T(a) + T(b)));
        public static Timecode operator -(Timecode a, Timecode b) => FromTicks(checked(T(a) - T(b)));
    }

    public class Marker
    {
        internal int _origIndex = -1;
        public Timecode Position { get; set; } = new Timecode();
        public string Label { get; set; } = "";
        public Marker() { }
        public Marker(Timecode pos, string label)
        {
            Position = pos ?? new Timecode();
            Label = label ?? "";
        }
    }

    // Fade (Scripting API Summary: Fade; FAQ p. 1038: FadeIn.Length).
    public sealed class Fade
    {
        public Timecode Length { get; set; } = new Timecode();
    }

    // Media, MediaStream, VideoStream, AudioStream, MediaStreams, MediaPool,
    // Take e Takes (Scripting API Summary). MediaPool.AddMedia e
    // new Media(path) importam um arquivo que ainda não está no projeto:
    // o Neoveg o abre na hora (Project.ImportMedia) e ele só entra no
    // projeto se o script terminar sem erro. O Neoveg tem no máximo um
    // stream de vídeo e um de áudio por mídia.
    public abstract class MediaStream
    {
        internal MediaStream(Media parent, int index)
        {
            Parent = parent;
            Index = index;
        }
        public Media Parent { get; }
        public abstract MediaType MediaType { get; }
        public int Index { get; }
        public Timecode Offset => new Timecode();
        public Timecode Length => Parent == null ? new Timecode() : Parent.Length;
        public override bool Equals(object obj) =>
            obj is MediaStream s && s.MediaType == MediaType && Equals(s.Parent, Parent);
        public override int GetHashCode() => (Parent?.GetHashCode() ?? 0) ^ (int)MediaType;
        public static bool operator ==(MediaStream a, MediaStream b) =>
            ReferenceEquals(a, b) || (a is not null && a.Equals(b));
        public static bool operator !=(MediaStream a, MediaStream b) => !(a == b);
    }

    public sealed class VideoStream : MediaStream
    {
        internal VideoStream(Media parent, int index) : base(parent, index) { }
        public override MediaType MediaType => MediaType.Video;
        public int Width => Parent?._width ?? 0;
        public int Height => Parent?._height ?? 0;
        public double FrameRate => Parent?._fps ?? 0.0;
    }

    public sealed class AudioStream : MediaStream
    {
        internal AudioStream(Media parent, int index) : base(parent, index) { }
        public override MediaType MediaType => MediaType.Audio;
    }

    public sealed class MediaStreams : List<MediaStream>
    {
        internal MediaStreams() { }
    }

    public sealed class Media
    {
        internal ulong _id;
        internal int _width;
        internal int _height;
        internal double _fps;
        private bool _generated;
        private string _name = "";

        internal Media(MediaSnap s)
        {
            Init(s?.id ?? 0, s?.path, s?.name, s != null && s.has_video, s != null && s.has_audio,
                s != null && s.generated, Timecode.FromTicks(s?.length ?? 0));
            _width = s?.width ?? 0;
            _height = s?.height ?? 0;
            _fps = s?.fps ?? 0.0;
        }

        // Media(path) e Media.CreateInstance(project, path): acham a mídia
        // do projeto pelo caminho ou a importam, como MediaPool.AddMedia.
        public Media(string path) : this(Project.s_current, path) { }

        private Media(Project project, string path)
        {
            var found = project?.MediaPool.Find(path) ?? project?.ImportMedia(path);
            if (found == null)
            {
                throw new NotSupportedException("a mídia '" + (path ?? "") + "' não está no projeto");
            }
            Init(found._id, found.FilePath, found._name, found.HasVideo(), found.HasAudio(),
                found._generated, found.Length);
            _width = found._width;
            _height = found._height;
            _fps = found._fps;
        }

        public static Media CreateInstance(Project project, string path) => new Media(project, path);

        private void Init(ulong id, string path, string name, bool video, bool audio, bool generated, Timecode length)
        {
            _id = id;
            FilePath = path ?? "";
            _name = name ?? "";
            _generated = generated;
            Length = length ?? new Timecode();
            Streams = new MediaStreams();
            if (video)
            {
                Streams.Add(new VideoStream(this, Streams.Count));
            }
            if (audio)
            {
                Streams.Add(new AudioStream(this, Streams.Count));
            }
        }

        public string FilePath { get; private set; } = "";
        public string KeyString => _generated ? _name : FilePath;
        public Timecode Length { get; private set; } = new Timecode();
        public MediaStreams Streams { get; private set; } = new MediaStreams();
        public bool HasVideo() => GetVideoStreamByIndex(0) != null;
        public bool HasAudio() => GetAudioStreamByIndex(0) != null;
        public bool IsGenerated() => _generated;
        public bool IsValid() => true;

        public int StreamCount(MediaType type)
        {
            int n = 0;
            foreach (var s in Streams)
            {
                if (s.MediaType == type)
                {
                    n++;
                }
            }
            return n;
        }

        public VideoStream GetVideoStreamByIndex(int index) => ByIndex(MediaType.Video, index) as VideoStream;

        public AudioStream GetAudioStreamByIndex(int index) => ByIndex(MediaType.Audio, index) as AudioStream;

        private MediaStream ByIndex(MediaType type, int index)
        {
            int k = 0;
            foreach (var s in Streams)
            {
                if (s.MediaType == type)
                {
                    if (k == index)
                    {
                        return s;
                    }
                    k++;
                }
            }
            return null;
        }

        // stream do tipo pedido mesmo que a mídia não o declare (snapshot
        // inconsistente): o take continua apontando para a mesma mídia
        internal MediaStream StreamFor(MediaType type)
        {
            var s = ByIndex(type, 0);
            if (s != null)
            {
                return s;
            }
            return type == MediaType.Video ? new VideoStream(this, 0) : (MediaStream)new AudioStream(this, 0);
        }

        public override bool Equals(object that) => that is Media m && m._id == _id;
        public override int GetHashCode() => _id.GetHashCode();
        public static bool operator ==(Media a, Media b) => ReferenceEquals(a, b) || (a is not null && a.Equals(b));
        public static bool operator !=(Media a, Media b) => !(a == b);

        internal bool Matches(string key)
        {
            if (string.IsNullOrEmpty(key))
            {
                return false;
            }
            if (_generated)
            {
                return string.Equals(_name, key, StringComparison.Ordinal);
            }
            var cmp = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
            return string.Equals(Normalize(FilePath), Normalize(key), cmp);
        }

        private static string Normalize(string p)
        {
            try
            {
                return Path.GetFullPath(p);
            }
            catch
            {
                return p;
            }
        }
    }

    public sealed class MediaPool : IEnumerable<Media>
    {
        private readonly List<Media> _items = new List<Media>();
        internal Project _owner;
        internal MediaPool() { }
        internal void AddInternal(Media m) => _items.Add(m);
        internal Media ById(ulong id) => _items.Find(m => m._id == id);
        public int Count => _items.Count;
        public bool IsReadOnly => true;
        public Media this[object key] => key is string s ? Find(s) : null;
        public Media Find(string path) => _items.Find(m => m.Matches(path));
        public bool Contains(object key) => key is string s && Find(s) != null;

        public Media AddMedia(string path) => AddMedia(path, false);

        public Media AddMedia(string path, bool addToSelectedBin)
        {
            var found = Find(path);
            if (found != null)
            {
                return found;
            }
            if (_owner == null)
            {
                throw new NotSupportedException("MediaPool.AddMedia: mídia sem projeto ('" + (path ?? "") + "')");
            }
            return _owner.ImportMedia(path);
        }

        public IEnumerator<Media> GetEnumerator() => _items.GetEnumerator();
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }

    // Take (Scripting API Summary: Take). Length é a duração do evento (o
    // trecho usado); AvailableLength é o que resta na mídia depois de Offset.
    public sealed class Take
    {
        internal TrackEvent _event;

        internal Take(MediaStream stream, string name)
        {
            MediaStream = stream;
            Name = name ?? "";
        }

        public Media Media => MediaStream?.Parent;
        public MediaStream MediaStream { get; }
        public string MediaPath => Media?.FilePath ?? "";
        public string Name { get; set; } = "";
        public int StreamIndex => MediaStream?.Index ?? -1;
        public Timecode Offset { get; set; } = new Timecode();
        public Timecode Length => _event?.Length ?? new Timecode();

        public Timecode AvailableLength
        {
            get
            {
                var len = Media?.Length ?? new Timecode();
                var off = Offset ?? new Timecode();
                return len > off ? len - off : new Timecode();
            }
        }

        public bool IsActive => _event != null && ReferenceEquals(_event.ActiveTake, this);
        public int Index => _event == null ? -1 : _event.Takes.IndexOf(this);
        public bool IsValid() => _event != null && _event.Takes.Contains(this);
    }

    // Takes (Scripting API Summary): remover um take muda o Index dos
    // seguintes, como no VEGAS.
    public sealed class Takes : List<Take>
    {
        internal Takes() { }
    }

    // TrackEvent / VideoEvent / AudioEvent (Scripting API Summary). Eventos novos
    // nascem por VideoTrack.AddVideoEvent / AudioTrack.AddAudioEvent e
    // precisam de um take (AddTake) com mídia do projeto: um evento sem take
    // recusa o script. Os construtores são internos.
    public abstract class TrackEvent
    {
        internal ulong _id;
        internal Track _track;
        public string Name { get; set; } = "";
        public Timecode Start { get; set; } = new Timecode();
        public Timecode Length { get; set; } = new Timecode();
        public Timecode End
        {
            get => Start + Length;
            set => Length = value - Start;
        }
        public bool Mute { get; set; }
        public bool Locked { get; set; }
        public bool Selected { get; set; }
        public Fade FadeIn { get; } = new Fade();
        public Fade FadeOut { get; } = new Fade();
        public int Index => _track == null ? -1 : _track.Events.IndexOf(this);
        public Track Track
        {
            get => _track;
            set
            {
                if (ReferenceEquals(value, _track))
                {
                    return;
                }
                _track?.Events.Remove(this);
                value?.Events.Add(this);
                _track = value;
            }
        }
        internal TrackEvent() { }
        internal bool _isNew;
        internal Take _active;
        public Takes Takes { get; } = new Takes();

        // ActiveTake: se o take ativo saiu da lista, o primeiro que sobrou
        // passa a ser o ativo.
        public Take ActiveTake
        {
            get
            {
                if (_active != null && Takes.Contains(_active))
                {
                    return _active;
                }
                return Takes.Count > 0 ? Takes[0] : null;
            }
            set
            {
                if (value == null || !Takes.Contains(value))
                {
                    throw new ArgumentException("ActiveTake: o take não pertence a este evento");
                }
                _active = value;
            }
        }

        public Take AddTake(MediaStream mediaStream) => AddTake(mediaStream, true, null);

        public Take AddTake(MediaStream mediaStream, bool makeActive) => AddTake(mediaStream, makeActive, null);

        public Take AddTake(MediaStream mediaStream, bool makeActive, string name)
        {
            if (mediaStream is null || mediaStream.Parent is null)
            {
                throw new ArgumentNullException(nameof(mediaStream));
            }
            var want = IsVideo() ? MediaType.Video : MediaType.Audio;
            if (mediaStream.MediaType != want)
            {
                throw new ArgumentException("AddTake: o stream precisa ser do mesmo tipo do evento (vídeo ou áudio)");
            }
            var t = new Take(mediaStream, name) { _event = this };
            Takes.Add(t);
            if (makeActive || Takes.Count == 1)
            {
                _active = t;
            }
            return t;
        }
        public abstract bool IsAudio();
        public abstract bool IsVideo();
        public bool IsValid() => _track != null;
    }

    public sealed class VideoEvent : TrackEvent
    {
        internal VideoEvent() { }
        public override bool IsAudio() => false;
        public override bool IsVideo() => true;
    }

    public sealed class AudioEvent : TrackEvent
    {
        internal AudioEvent() { }
        public override bool IsAudio() => true;
        public override bool IsVideo() => false;
    }

    // TrackEvents (Scripting API Summary: coleção de eventos da trilha).
    public sealed class TrackEvents : List<TrackEvent>
    {
        internal TrackEvents() { }
    }

    public class Track
    {
        internal int _origIndex = -1;
        public int Index { get; internal set; }
        public string Name { get; set; } = "";
        public bool Mute { get; set; }
        public bool Solo { get; set; }
        public bool Selected { get; set; }
        public TrackEvents Events { get; } = new TrackEvents();
        public virtual bool IsVideo() => false;
        public virtual bool IsAudio() => false;
        // Track.IsValid (Scripting API Summary: Track): está no projeto.
        internal Project _project;
        public bool IsValid() => _project != null && _project.Tracks.Contains(this);
    }

    public sealed class VideoTrack : Track
    {
        public override bool IsVideo() => true;

        // VideoTrack.AddVideoEvent (Scripting API Summary).
        public VideoEvent AddVideoEvent() => AddVideoEvent(new Timecode(), new Timecode());

        public VideoEvent AddVideoEvent(Timecode start, Timecode length)
        {
            var ev = new VideoEvent { _isNew = true, _track = this };
            ev.Start = start ?? new Timecode();
            ev.Length = length ?? new Timecode();
            Events.Add(ev);
            return ev;
        }
    }

    public sealed class AudioTrack : Track
    {
        public override bool IsAudio() => true;

        // AudioTrack.AddAudioEvent (Scripting API Summary).
        public AudioEvent AddAudioEvent() => AddAudioEvent(new Timecode(), new Timecode());

        public AudioEvent AddAudioEvent(Timecode start, Timecode length)
        {
            var ev = new AudioEvent { _isNew = true, _track = this };
            ev.Start = start ?? new Timecode();
            ev.Length = length ?? new Timecode();
            Events.Add(ev);
            return ev;
        }
    }

    // TransportControl (Scripting API Summary): cursor e seleção de tempo.
    public sealed class TransportControl
    {
        public Timecode CursorPosition { get; set; } = new Timecode();
        public Timecode SelectionStart { get; set; } = new Timecode();
        public Timecode SelectionLength { get; set; } = new Timecode();
    }

    public sealed class Project
    {
        public List<Track> Tracks { get; } = new List<Track>();
        public List<Marker> Markers { get; } = new List<Marker>();
        public List<string> AudioBuses { get; } = new List<string>();
        // Project.MediaPool (Scripting API Summary).
        public MediaPool MediaPool { get; } = new MediaPool();
        // projeto deste script, para Media(path) achar a mídia
        internal static Project s_current;

        // função do Neoveg que abre um arquivo de mídia enquanto o script
        // roda (0 = este host não importa) e a execução a que pertence
        internal ulong _probeFn;
        internal ulong _probeToken;

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate int MediaProbeFn(ulong token, IntPtr path, int pathLen, IntPtr outBuf, int outCap);

        // Abre o arquivo pelo Neoveg e põe a mídia no MediaPool deste script.
        // A documentação não diz o que AddMedia lança numa falha: aqui é
        // IOException, com o motivo dado pelo Neoveg.
        internal Media ImportMedia(string path)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                throw new ArgumentException("caminho de mídia vazio");
            }
            if (_probeFn == 0)
            {
                throw new NotSupportedException("este host do Neoveg não importa mídia por script ('" + path + "')");
            }
            var fn = Marshal.GetDelegateForFunctionPointer<MediaProbeFn>(new IntPtr(unchecked((long)_probeFn)));
            byte[] raw = Encoding.UTF8.GetBytes(path);
            IntPtr pathPtr = Marshal.AllocHGlobal(Math.Max(1, raw.Length));
            IntPtr outPtr = IntPtr.Zero;
            string json = null;
            try
            {
                Marshal.Copy(raw, 0, pathPtr, raw.Length);
                int cap = 16 * 1024;
                for (int attempt = 0; attempt < 2 && json == null; attempt++)
                {
                    outPtr = Marshal.AllocHGlobal(cap);
                    int n = fn(_probeToken, pathPtr, raw.Length, outPtr, cap);
                    if (n >= 0 && n <= cap)
                    {
                        json = Marshal.PtrToStringUTF8(outPtr, n);
                    }
                    else if (n < 0 && n != int.MinValue && -n > cap && -n <= 1024 * 1024)
                    {
                        // resposta maior que o espaço: tenta de novo com o tamanho pedido
                        cap = -n;
                    }
                    else
                    {
                        break;
                    }
                    Marshal.FreeHGlobal(outPtr);
                    outPtr = IntPtr.Zero;
                }
            }
            finally
            {
                if (outPtr != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(outPtr);
                }
                Marshal.FreeHGlobal(pathPtr);
            }
            if (json == null)
            {
                throw new InvalidOperationException("o Neoveg não respondeu à importação de '" + path + "'");
            }
            var reply = JsonSerializer.Deserialize<ProbeReplySnap>(json);
            if (reply == null || !reply.ok || reply.media == null)
            {
                throw new IOException("MediaPool.AddMedia: não foi possível importar '" + path + "': " + (reply?.error ?? "resposta inválida"));
            }
            var existing = MediaPool.ById(reply.media.id);
            if (existing != null)
            {
                return existing;
            }
            var m = new Media(reply.media);
            MediaPool.AddInternal(m);
            return m;
        }

        public VideoTrack AddVideoTrack(string name = null)
        {
            var t = new VideoTrack { Index = Tracks.Count, Name = name ?? "", _project = this };
            Tracks.Add(t);
            return t;
        }

        public AudioTrack AddAudioTrack(string name = null)
        {
            var t = new AudioTrack { Index = Tracks.Count, Name = name ?? "", _project = this };
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

        // Vegas.Cursor (Scripting API Summary: Vegas).
        public Timecode Cursor
        {
            get => Transport.CursorPosition;
            set => Transport.CursorPosition = value;
        }

        internal Vegas(InputSnap snap)
        {
            if (snap.fps_num > 0 && snap.fps_den > 0)
            {
                Timecode.FpsNum = snap.fps_num;
                Timecode.FpsDen = snap.fps_den;
            }
            Transport.CursorPosition = Timecode.FromTicks(snap.cursor_ticks);
            Transport.SelectionStart = Timecode.FromTicks(snap.sel_start_ticks);
            Transport.SelectionLength = Timecode.FromTicks(snap.sel_length_ticks);
            var selected = new HashSet<ulong>(snap.selected_events ?? new List<ulong>());
            Project.s_current = Project;
            Project.MediaPool._owner = Project;
            Project._probeFn = snap.probe_fn;
            Project._probeToken = snap.probe_token;
            foreach (var ms in snap.media ?? new List<MediaSnap>())
            {
                Project.MediaPool.AddInternal(new Media(ms));
            }
            if (snap.tracks != null)
            {
                foreach (var t in snap.tracks)
                {
                    Track tr = t.is_video ? new VideoTrack() : new AudioTrack();
                    tr._origIndex = t.index;
                    tr._project = Project;
                    tr.Index = t.index;
                    tr.Name = t.name ?? "";
                    tr.Mute = t.mute;
                    tr.Solo = t.solo;
                    tr.Selected = snap.selected_track == t.index;
                    foreach (var e in t.events ?? new List<EventSnap>())
                    {
                        TrackEvent ev = t.is_video ? new VideoEvent() : new AudioEvent();
                        ev._id = e.id;
                        ev._track = tr;
                        ev.Name = e.name ?? "";
                        ev.Start = Timecode.FromTicks(e.start);
                        ev.Length = Timecode.FromTicks(e.length);
                        ev.Mute = e.mute;
                        ev.Locked = e.locked;
                        ev.Selected = selected.Contains(e.id);
                        ev.FadeIn.Length = Timecode.FromTicks(e.fade_in);
                        ev.FadeOut.Length = Timecode.FromTicks(e.fade_out);
                        var kind = t.is_video ? MediaType.Video : MediaType.Audio;
                        foreach (var ts in e.takes ?? new List<TakeSnap>())
                        {
                            var m = Project.MediaPool.ById(ts.media_id)
                                ?? new Media(new MediaSnap { id = ts.media_id });
                            var tk = new Take(m.StreamFor(kind), ts.name) { _event = ev };
                            tk.Offset = Timecode.FromTicks(ts.offset);
                            ev.Takes.Add(tk);
                        }
                        if (e.active_take >= 0 && e.active_take < ev.Takes.Count)
                        {
                            ev._active = ev.Takes[e.active_take];
                        }
                        tr.Events.Add(ev);
                    }
                    Project.Tracks.Add(tr);
                }
            }
            if (snap.markers != null)
            {
                for (int i = 0; i < snap.markers.Count; i++)
                {
                    var m = snap.markers[i];
                    var mk = new Marker(Timecode.FromTicks(m.position_ticks), m.label)
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

        private static long Ticks(Timecode t) => t == null ? 0 : t._ticks;

        private static List<TakeOut> TakesOut(TrackEvent ev)
        {
            var list = new List<TakeOut>();
            foreach (var t in ev.Takes)
            {
                if (t == null)
                {
                    throw new InvalidOperationException("take nulo na lista de takes");
                }
                list.Add(new TakeOut
                {
                    media_id = t.Media?._id ?? 0,
                    offset = Ticks(t.Offset),
                    name = t.Name ?? ""
                });
            }
            return list;
        }

        private static bool SameTakes(List<TakeOut> now, List<TakeSnap> old)
        {
            old ??= new List<TakeSnap>();
            if (now.Count != old.Count)
            {
                return false;
            }
            for (int i = 0; i < now.Count; i++)
            {
                if (now[i].media_id != old[i].media_id
                    || now[i].offset != old[i].offset
                    || (now[i].name ?? "") != (old[i].name ?? ""))
                {
                    return false;
                }
            }
            return true;
        }

        // Devolve a lista FINAL de trilhas (com os eventos de cada uma, por
        // id), de marcadores, a seleção e o cursor. Cada item diz de onde
        // veio e só o que mudou, para nada se perder em silêncio (§0).
        internal OutputSnap BuildResult(InputSnap orig)
        {
            var tracksOut = new List<TrackOut>();
            var markersOut = new List<MarkerOut>();
            var addedBuses = new List<string>();
            var selectedEvents = new List<ulong>();

            var origTracks = orig.tracks ?? new List<TrackSnap>();
            var origEvents = new Dictionary<ulong, EventSnap>();
            foreach (var t in origTracks)
            {
                foreach (var e in t.events ?? new List<EventSnap>())
                {
                    origEvents[e.id] = e;
                }
            }
            var seenTracks = new HashSet<int>();
            var seenEvents = new HashSet<ulong>();
            var seenObjects = new HashSet<TrackEvent>(ReferenceEqualityComparer.Instance);
            foreach (var cur in Project.Tracks)
            {
                if (cur == null)
                {
                    continue;
                }
                var evOut = new List<EventOut>();
                foreach (var ev in cur.Events)
                {
                    if (ev == null)
                    {
                        continue;
                    }
                    if (!seenObjects.Add(ev))
                    {
                        throw new InvalidOperationException(
                            "o mesmo evento aparece mais de uma vez nas trilhas");
                    }
                    long st = Ticks(ev.Start), ln = Ticks(ev.Length);
                    long fi = Ticks(ev.FadeIn.Length), fo = Ticks(ev.FadeOut.Length);
                    var takesNow = TakesOut(ev);
                    int activeNow = ev.Takes.IndexOf(ev.ActiveTake);
                    if (ev._isNew)
                    {
                        // evento criado pelo script: vai inteiro, sem id
                        evOut.Add(new EventOut
                        {
                            is_new = true,
                            selected = ev.Selected,
                            name = string.IsNullOrWhiteSpace(ev.Name) ? null : ev.Name,
                            start = st,
                            length = ln,
                            mute = ev.Mute ? true : (bool?)null,
                            locked = ev.Locked ? true : (bool?)null,
                            fade_in = fi != 0 ? fi : (long?)null,
                            fade_out = fo != 0 ? fo : (long?)null,
                            takes = takesNow,
                            active_take = activeNow
                        });
                        continue;
                    }
                    if (!seenEvents.Add(ev._id))
                    {
                        throw new InvalidOperationException(
                            "o mesmo evento aparece mais de uma vez nas trilhas");
                    }
                    if (!origEvents.TryGetValue(ev._id, out var oe))
                    {
                        throw new InvalidOperationException("evento desconhecido");
                    }
                    if (ev.Selected)
                    {
                        selectedEvents.Add(ev._id);
                    }
                    bool takesChanged = !SameTakes(takesNow, oe.takes) || activeNow != oe.active_take;
                    evOut.Add(new EventOut
                    {
                        id = ev._id,
                        name = (ev.Name ?? "") != (oe.name ?? "") ? (ev.Name ?? "") : null,
                        start = st != oe.start ? st : (long?)null,
                        length = ln != oe.length ? ln : (long?)null,
                        mute = ev.Mute != oe.mute ? ev.Mute : (bool?)null,
                        locked = ev.Locked != oe.locked ? ev.Locked : (bool?)null,
                        fade_in = fi != oe.fade_in ? fi : (long?)null,
                        fade_out = fo != oe.fade_out ? fo : (long?)null,
                        takes = takesChanged ? takesNow : null,
                        active_take = takesChanged ? activeNow : (int?)null
                    });
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
                        solo = cur.Solo != old.solo ? cur.Solo : (bool?)null,
                        selected = cur.Selected,
                        events = evOut
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
                        solo = cur.Solo ? true : (bool?)null,
                        selected = cur.Selected,
                        events = evOut
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
                long ticks = Ticks(mk.Position);
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
                        ticks = ticks != old.position_ticks ? ticks : (long?)null,
                        label = label != (old.label ?? "") ? label : null
                    });
                }
                else
                {
                    markersOut.Add(new MarkerOut
                    {
                        orig = -1,
                        ticks = ticks,
                        label = label
                    });
                }
            }

            for (int i = _origBusCount; i < Project.AudioBuses.Count; i++)
            {
                var bus = Project.AudioBuses[i];
                addedBuses.Add(string.IsNullOrWhiteSpace(bus) ? null : bus);
            }

            long cur_t = Ticks(Transport.CursorPosition);
            long sel_s = Ticks(Transport.SelectionStart), sel_l = Ticks(Transport.SelectionLength);
            return new OutputSnap
            {
                ok = true,
                protocol = 4,
                messages = _messages,
                commands = _commands,
                tracks = tracksOut,
                added_buses = addedBuses,
                markers = markersOut,
                selected_events = selectedEvents,
                cursor_ticks = cur_t != orig.cursor_ticks ? cur_t : (long?)null,
                sel_start_ticks = sel_s != orig.sel_start_ticks || sel_l != orig.sel_length_ticks ? sel_s : (long?)null,
                sel_length_ticks = sel_s != orig.sel_start_ticks || sel_l != orig.sel_length_ticks ? sel_l : (long?)null
            };
        }
    }

    internal sealed class TakeSnap
    {
        public ulong media_id { get; set; }
        public long offset { get; set; }
        public string name { get; set; }
    }

    internal sealed class MediaSnap
    {
        public ulong id { get; set; }
        public string path { get; set; }
        public string name { get; set; }
        public bool has_video { get; set; }
        public bool has_audio { get; set; }
        public bool generated { get; set; }
        public long length { get; set; }
        public int width { get; set; }
        public int height { get; set; }
        public double fps { get; set; }
    }

    internal sealed class ProbeReplySnap
    {
        public bool ok { get; set; }
        public string error { get; set; }
        public MediaSnap media { get; set; }
    }

    internal sealed class TakeOut
    {
        public ulong media_id { get; set; }
        public long offset { get; set; }
        public string name { get; set; }
    }

    internal sealed class EventSnap
    {
        public ulong id { get; set; }
        public string name { get; set; }
        public long start { get; set; }
        public long length { get; set; }
        public bool mute { get; set; }
        public bool locked { get; set; }
        public long fade_in { get; set; }
        public long fade_out { get; set; }
        public List<TakeSnap> takes { get; set; }
        public int active_take { get; set; }
    }

    internal sealed class TrackSnap
    {
        public int index { get; set; }
        public string name { get; set; }
        public bool is_video { get; set; }
        public bool mute { get; set; }
        public bool solo { get; set; }
        public List<EventSnap> events { get; set; }
    }

    internal sealed class MarkerSnap
    {
        public long position_ticks { get; set; }
        public string label { get; set; }
    }

    internal sealed class InputSnap
    {
        public string result_path { get; set; }
        public string target_assembly_path { get; set; }
        public long fps_num { get; set; }
        public long fps_den { get; set; }
        public long cursor_ticks { get; set; }
        public long sel_start_ticks { get; set; }
        public long sel_length_ticks { get; set; }
        public int selected_track { get; set; } = -1;
        public List<ulong> selected_events { get; set; }
        public List<TrackSnap> tracks { get; set; }
        public List<MarkerSnap> markers { get; set; }
        public List<string> audio_buses { get; set; }
        public List<MediaSnap> media { get; set; }
        public ulong probe_fn { get; set; }
        public ulong probe_token { get; set; }
    }

    internal sealed class EventOut
    {
        public ulong id { get; set; }
        public string name { get; set; }
        public long? start { get; set; }
        public long? length { get; set; }
        public bool? mute { get; set; }
        public bool? locked { get; set; }
        public long? fade_in { get; set; }
        public long? fade_out { get; set; }
        public bool is_new { get; set; }
        public bool selected { get; set; }
        public List<TakeOut> takes { get; set; }
        public int? active_take { get; set; }
    }

    internal sealed class TrackOut
    {
        public int orig { get; set; }
        public bool is_video { get; set; }
        public string name { get; set; }
        public bool? mute { get; set; }
        public bool? solo { get; set; }
        public bool selected { get; set; }
        public List<EventOut> events { get; set; } = new List<EventOut>();
    }

    internal sealed class MarkerOut
    {
        public int orig { get; set; }
        public long? ticks { get; set; }
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
        public List<ulong> selected_events { get; set; } = new List<ulong>();
        public long? cursor_ticks { get; set; }
        public long? sel_start_ticks { get; set; }
        public long? sel_length_ticks { get; set; }
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

        // No Windows o script roda numa thread STA, como no VEGAS: os diálogos
        // do WinForms (OpenFileDialog, FolderBrowserDialog...) exigem STA. A
        // exceção do script é relançada igual, com a pilha original.
        private static bool InvokeUserEntryPoint(Assembly asm, Vegas vegas)
        {
            if (!OperatingSystem.IsWindows()
                || System.Threading.Thread.CurrentThread.GetApartmentState() == System.Threading.ApartmentState.STA)
            {
                return InvokeUserEntryPointCore(asm, vegas);
            }
            bool invoked = false;
            ExceptionDispatchInfo failure = null;
            var sta = new System.Threading.Thread(() =>
            {
                try
                {
                    invoked = InvokeUserEntryPointCore(asm, vegas);
                }
                catch (Exception ex)
                {
                    failure = ExceptionDispatchInfo.Capture(ex);
                }
            });
            sta.SetApartmentState(System.Threading.ApartmentState.STA);
            sta.IsBackground = true;
            sta.Start();
            sta.Join();
            failure?.Throw();
            return invoked;
        }

        private static bool InvokeUserEntryPointCore(Assembly asm, Vegas vegas)
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

fn media_snap(m: &MediaItem) -> ScriptMediaSnap {
    let (path, generated) = match &m.source {
        MediaSource::File { path } => (path.clone(), false),
        MediaSource::ImageSequence { paths, .. } => {
            (paths.first().cloned().unwrap_or_default(), false)
        }
        MediaSource::Generated(_) => (String::new(), true),
    };
    ScriptMediaSnap {
        id: m.id,
        path,
        name: m.name.clone(),
        has_video: m.has_video,
        has_audio: m.has_audio,
        generated,
        length: m.duration.map_or(0, |d| d.0.max(0)),
        width: m.width,
        height: m.height,
        fps: m
            .fps
            .map_or(0.0, |f| f64::from(f.num) / f64::from(f.den.max(1))),
    }
}

// ---------------------------------------------------------------------------
// Importação de mídia durante o script (MediaPool.AddMedia, new Media(path))
// ---------------------------------------------------------------------------

/// Limite de arquivos que um script importa numa execução.
const MAX_SCRIPT_IMPORTS: usize = 64;
/// Limite do caminho recebido do script, em bytes.
pub(crate) const MAX_PROBE_PATH_BYTES: usize = 32 * 1024;

/// Mídia importada por execução de script. Só entra no projeto se o script
/// terminar sem erro (§0: nada muda num script que falha).
static PROBE_RUNS: Mutex<BTreeMap<u64, Vec<MediaItem>>> = Mutex::new(BTreeMap::new());

fn probe_runs() -> MutexGuard<'static, BTreeMap<u64, Vec<MediaItem>>> {
    PROBE_RUNS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Registro de uma execução; removido ao sair, com erro ou não.
struct ProbeRun(u64);

impl ProbeRun {
    fn start(token: u64) -> Self {
        probe_runs().insert(token, Vec::new());
        Self(token)
    }

    fn token(&self) -> u64 {
        self.0
    }

    fn take(&self) -> Vec<MediaItem> {
        probe_runs()
            .get_mut(&self.0)
            .map(std::mem::take)
            .unwrap_or_default()
    }
}

impl Drop for ProbeRun {
    fn drop(&mut self) {
        probe_runs().remove(&self.0);
    }
}

#[derive(Serialize)]
struct ProbeReply {
    ok: bool,
    error: Option<String>,
    media: Option<ScriptMediaSnap>,
}

/// Abre `path` como mídia para a execução `token` e responde em JSON
/// (`ProbeReplySnap` na ponte C#). Chamado pela fronteira FFI em
/// `dotnet_host`, na thread do host .NET, enquanto o script roda.
pub(crate) fn probe_media_for_script(token: u64, path: &[u8]) -> Vec<u8> {
    let reply = match probe_media_inner(token, path) {
        Ok(snap) => ProbeReply {
            ok: true,
            error: None,
            media: Some(snap),
        },
        Err(e) => ProbeReply {
            ok: false,
            error: Some(e),
            media: None,
        },
    };
    serde_json::to_vec(&reply).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"falha ao serializar a resposta da importação"}"#
            .as_bytes()
            .to_vec()
    })
}

fn probe_media_inner(token: u64, raw: &[u8]) -> Result<ScriptMediaSnap, String> {
    if raw.len() > MAX_PROBE_PATH_BYTES {
        return Err("caminho de mídia longo demais".into());
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| "caminho de mídia não é UTF-8 válido".to_string())?
        .trim();
    if text.is_empty() {
        return Err("caminho de mídia vazio".into());
    }
    let path = std::path::absolute(Path::new(text)).unwrap_or_else(|_| PathBuf::from(text));
    let key = path.to_string_lossy().to_string();
    {
        let runs = probe_runs();
        let Some(items) = runs.get(&token) else {
            return Err("execução de script desconhecida".into());
        };
        let same = items
            .iter()
            .find(|m| matches!(&m.source, MediaSource::File { path: p } if *p == key));
        if let Some(m) = same {
            return Ok(media_snap(m));
        }
        if items.len() >= MAX_SCRIPT_IMPORTS {
            return Err(format!(
                "o script passou do limite de {MAX_SCRIPT_IMPORTS} mídias importadas"
            ));
        }
    }
    if !path.is_file() {
        return Err(format!("arquivo não encontrado: {key}"));
    }
    // o lock não fica preso enquanto o arquivo é aberto
    let imp = media::import_file(&path).map_err(|e| format!("{e:#}"))?;
    let snap = media_snap(&imp.item);
    let mut runs = probe_runs();
    let Some(items) = runs.get_mut(&token) else {
        return Err("execução de script desconhecida".into());
    };
    if items.len() >= MAX_SCRIPT_IMPORTS {
        return Err(format!(
            "o script passou do limite de {MAX_SCRIPT_IMPORTS} mídias importadas"
        ));
    }
    items.push(imp.item);
    Ok(snap)
}

fn build_snapshot(
    session: &EditorSession,
    result_path: &Path,
    target_assembly: Option<&Path>,
    probe_token: u64,
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
            events: t
                .events
                .iter()
                .map(|e| ScriptEventSnap {
                    id: e.id,
                    name: e.name.clone(),
                    start: e.start.0,
                    length: e.length.0,
                    mute: e.mute,
                    locked: e.locked,
                    fade_in: e.fade_in.len.0,
                    fade_out: e.fade_out.len.0,
                    takes: takes::takes(e)
                        .into_iter()
                        .map(|t| ScriptTakeSnap {
                            media_id: t.media_id,
                            offset: t.offset.0,
                            name: t.name,
                        })
                        .collect(),
                    active_take: takes::active_index(e),
                })
                .collect(),
        })
        .collect();
    let media = session.project.media.iter().map(media_snap).collect();
    let markers = session
        .project
        .markers
        .iter()
        .map(|m| ScriptMarkerSnap {
            position_ticks: m.t.0,
            label: m.label.clone(),
        })
        .collect();
    let audio_buses = session
        .project
        .audio_buses
        .iter()
        .map(|b| b.name.clone())
        .collect();
    let (sel_start, sel_len) = session
        .time_sel
        .map_or((0, 0), |(a, b)| (a.0, (b - a).0.max(0)));
    let mut selected_events: Vec<u64> = session.sel_events.iter().copied().collect();
    selected_events.sort_unstable();
    let fps = session.project.settings.fps;
    let payload = ScriptInputPayload {
        result_path: result_path.to_string_lossy().to_string(),
        target_assembly_path: target_assembly.map(|p| p.to_string_lossy().to_string()),
        fps_num: i64::from(fps.num.max(1)),
        fps_den: i64::from(fps.den.max(1)),
        cursor_ticks: session.cursor.0,
        sel_start_ticks: sel_start,
        sel_length_ticks: sel_len,
        selected_track: session
            .sel_track
            .and_then(|i| i64::try_from(i).ok())
            .unwrap_or(-1),
        selected_events,
        tracks,
        markers,
        audio_buses,
        media,
        probe_fn: dotnet_host::media_probe_fn_addr(),
        probe_token,
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
        cs_project_xml(&env.tfm, env.use_winforms()),
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

/// A lista final é a original, na mesma ordem, com os mesmos eventos e sem
/// mudanças?
fn tracks_unchanged(out: &[ScriptTrackOut], old: &[Track]) -> bool {
    out.len() == old.len()
        && out.iter().zip(old).enumerate().all(|(i, (t, o))| {
            usize::try_from(t.orig).ok() == Some(i)
                && t.name.is_none()
                && t.mute.is_none()
                && t.solo.is_none()
                && t.events.len() == o.events.len()
                && t.events
                    .iter()
                    .zip(&o.events)
                    .all(|(e, oe)| e.id == oe.id && e.unchanged())
        })
}

fn markers_unchanged(out: &[ScriptMarkerOut], n: usize) -> bool {
    out.len() == n
        && out.iter().enumerate().all(|(i, m)| {
            usize::try_from(m.orig).ok() == Some(i) && m.ticks.is_none() && m.label.is_none()
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

fn bad(msg: impl Into<String>) -> NeovegError {
    NeovegError::InvalidInput(format!("script .NET: {}", msg.into()))
}

/// Ticks informados pelo script, sem valor negativo.
fn script_ticks(v: i64, what: &str) -> Result<Ticks, NeovegError> {
    if v < 0 {
        return Err(bad(format!("{what} negativo")));
    }
    Ok(Ticks::from_raw(v))
}

/// Aplica as mudanças do script num evento, validando os valores (§0.4):
/// nada é cortado em silêncio; um valor inválido recusa o script inteiro.
fn apply_event_changes(
    ev: &mut neoveg_engine::model::Event,
    o: &ScriptEventOut,
) -> Result<(), NeovegError> {
    if let Some(n) = &o.name {
        ev.name = truncate_str(n.trim(), 256);
    }
    if let Some(s) = o.start {
        ev.start = script_ticks(s, "início do evento")?;
    }
    if let Some(l) = o.length {
        if l <= 0 {
            return Err(bad("evento com duração zero ou negativa"));
        }
        ev.length = Ticks::from_raw(l);
    }
    if ev.start.0.checked_add(ev.length.0).is_none() {
        return Err(bad("fim do evento fora do limite"));
    }
    if let Some(m) = o.mute {
        ev.mute = m;
    }
    if let Some(k) = o.locked {
        ev.locked = k;
    }
    if let Some(f) = o.fade_in {
        ev.fade_in.len = script_ticks(f, "fade in")?;
    }
    if let Some(f) = o.fade_out {
        ev.fade_out.len = script_ticks(f, "fade out")?;
    }
    if ev.fade_in.len > ev.length || ev.fade_out.len > ev.length {
        return Err(bad("fade maior que o evento"));
    }
    Ok(())
}

/// Takes informados pelo script, validados (§0.4): a mídia existe e é do
/// tipo da trilha, offset não negativo, no máximo [`takes::MAX_TAKES`].
fn script_takes(
    list: &[ScriptTakeOut],
    kind: TrackKind,
    media: &[MediaItem],
) -> Result<Vec<Take>, NeovegError> {
    if list.is_empty() {
        return Err(bad(
            "evento sem take: use AddTake com uma mídia que está no projeto",
        ));
    }
    if list.len() > takes::MAX_TAKES {
        return Err(bad(format!(
            "mais de {} takes num evento",
            takes::MAX_TAKES
        )));
    }
    list.iter()
        .map(|t| {
            let Some(m) = media.iter().find(|m| m.id == t.media_id) else {
                return Err(bad("take com uma mídia que não está no projeto"));
            };
            let fits = match kind {
                TrackKind::Video => m.has_video,
                TrackKind::Audio => m.has_audio,
            };
            if !fits {
                return Err(bad("take de áudio em evento de vídeo, ou o contrário"));
            }
            Ok(Take {
                media_id: m.id,
                offset: script_ticks(t.offset, "offset do take")?,
                name: truncate_str(t.name.trim(), 256),
            })
        })
        .collect()
}

/// Índice do take ativo informado pelo script, dentro da lista.
fn script_active(a: Option<i64>, n: usize) -> Result<usize, NeovegError> {
    let a = a.unwrap_or(0);
    usize::try_from(a)
        .ok()
        .filter(|&i| i < n)
        .ok_or_else(|| bad("ActiveTake fora da lista de takes do evento"))
}

/// Cria o evento que o script adicionou (AddVideoEvent/AddAudioEvent).
fn new_script_event(
    eo: &ScriptEventOut,
    kind: TrackKind,
    media: &[MediaItem],
) -> Result<neoveg_engine::model::Event, NeovegError> {
    let list = script_takes(eo.takes.as_deref().unwrap_or(&[]), kind, media)?;
    let active = script_active(eo.active_take, list.len())?;
    let m = list
        .get(active)
        .and_then(|t| media.iter().find(|m| m.id == t.media_id))
        .ok_or_else(|| NeovegError::Internal("take ativo sem mídia".into()))?;
    let start = script_ticks(eo.start.unwrap_or(0), "início do evento")?;
    let mut ev = neoveg_engine::model::Event::new(m, start, Ticks::from_raw(1));
    let o = ScriptEventOut {
        name: eo.name.clone().filter(|n| !n.trim().is_empty()),
        length: Some(eo.length.unwrap_or(0)),
        mute: eo.mute,
        locked: eo.locked,
        fade_in: eo.fade_in,
        fade_out: eo.fade_out,
        ..Default::default()
    };
    apply_event_changes(&mut ev, &o)?;
    if !takes::replace_takes(&mut ev, list, active) {
        return Err(NeovegError::Internal("lista de takes recusada".into()));
    }
    Ok(ev)
}

/// Remonta as trilhas na ordem final do script, com os eventos de cada uma.
/// Trilhas e eventos que ficaram de fora foram apagados pelo script; um
/// evento só pode ficar numa trilha do mesmo tipo (vídeo ou áudio) e
/// aparecer uma vez (§3.1). Trilhas e eventos existentes mantêm id e tudo o
/// que o script não mudou.
/// Devolve também os ids dos eventos criados que o script deixou
/// selecionados.
fn rebuild_tracks(
    old: &[Track],
    out: Vec<ScriptTrackOut>,
    media: &[MediaItem],
) -> Result<(Vec<Track>, Vec<u64>), NeovegError> {
    use std::collections::HashMap;
    let mut pool: HashMap<u64, (TrackKind, &neoveg_engine::model::Event)> = HashMap::new();
    for t in old {
        for e in &t.events {
            pool.insert(e.id, (t.kind, e));
        }
    }
    let mut used_events: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut used = vec![false; old.len()];
    let mut tracks: Vec<Track> = Vec::with_capacity(out.len());
    let mut new_selected = Vec::new();
    for t in out {
        let mut tr = match take_orig(t.orig, &mut used, "trilha")? {
            Some(i) => {
                let Some(src) = old.get(i) else {
                    return Err(NeovegError::Internal(
                        "índice de trilha fora da lista".into(),
                    ));
                };
                let mut tr = src.clone();
                if let Some(name) = &t.name {
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
                    .as_deref()
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
        let mut events = Vec::with_capacity(t.events.len());
        for eo in &t.events {
            if eo.is_new {
                let ev = new_script_event(eo, tr.kind, media)?;
                if eo.selected {
                    new_selected.push(ev.id);
                }
                events.push(ev);
                continue;
            }
            let Some((kind, src)) = pool.get(&eo.id) else {
                return Err(bad(format!("evento desconhecido: {}", eo.id)));
            };
            if !used_events.insert(eo.id) {
                return Err(bad(format!("o evento {} aparece mais de uma vez", eo.id)));
            }
            if *kind != tr.kind {
                return Err(bad("evento de vídeo em trilha de áudio, ou o contrário"));
            }
            let mut ev = (*src).clone();
            apply_event_changes(&mut ev, eo)?;
            if let Some(list) = &eo.takes {
                let list = script_takes(list, tr.kind, media)?;
                let a = script_active(eo.active_take, list.len())?;
                if !takes::replace_takes(&mut ev, list, a) {
                    return Err(NeovegError::Internal("lista de takes recusada".into()));
                }
            } else if eo.active_take.is_some() {
                let a = script_active(eo.active_take, takes::take_count(&ev))?;
                takes::set_active(&mut ev, a);
            }
            events.push(ev);
        }
        tr.events = events;
        tracks.push(tr);
    }
    Ok((tracks, new_selected))
}

fn marker_ticks(ticks: i64) -> Result<Ticks, NeovegError> {
    let t = script_ticks(ticks, "tempo do marcador")?;
    Ok(t.clamp_safe(Ticks::ZERO, Ticks::from_secs_i64(48 * 3600)))
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
                if let Some(t) = m.ticks {
                    mk.t = marker_ticks(t)?;
                }
                if let Some(l) = m.label {
                    mk.label = marker_label(&l);
                }
                markers.push(mk);
            }
            None => markers.push(Marker {
                id: neoveg_engine::model::new_id(),
                t: marker_ticks(m.ticks.unwrap_or(0))?,
                label: marker_label(m.label.as_deref().unwrap_or("")),
            }),
        }
    }
    Ok(markers)
}

fn apply_script_output(
    out: ScriptOutputPayload,
    imported: Vec<MediaItem>,
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
    let n_events: usize = session.project.tracks.iter().map(|t| t.events.len()).sum();
    let out_events: usize = out.tracks.iter().map(|t| t.events.len()).sum();
    if out.tracks.len() > n_tracks.saturating_add(MAX_SCRIPT_ITEMS)
        || out_events > n_events.saturating_add(MAX_SCRIPT_ITEMS)
        || out.selected_events.len() > n_events
        || out.added_buses.len() > MAX_SCRIPT_ITEMS
        || out.markers.len() > n_markers.saturating_add(MAX_SCRIPT_ITEMS)
        || out.commands.len() > MAX_SCRIPT_ITEMS
        || out.messages.len() > MAX_SCRIPT_ITEMS
    {
        return Err(NeovegError::InvalidInput(format!(
            "saída do script excede o limite de {MAX_SCRIPT_ITEMS} operações por execução"
        )));
    }

    let tracks_changed = !tracks_unchanged(&out.tracks, &session.project.tracks);
    let markers_changed = !markers_unchanged(&out.markers, n_markers);
    let has_project_mutations =
        tracks_changed || !out.added_buses.is_empty() || markers_changed || !imported.is_empty();
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

    // seleção de trilha: o Neoveg seleciona uma por vez
    let selected_tracks: Vec<usize> = out
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.selected)
        .map(|(i, _)| i)
        .collect();
    // a mídia importada entra antes das trilhas, para os takes novos a acharem
    for item in imported {
        if !staged.project.media.iter().any(|m| m.id == item.id) {
            staged.project.media.push(item);
        }
    }
    let mut new_selected = Vec::new();
    if tracks_changed {
        let (tracks, sel) =
            rebuild_tracks(&staged.project.tracks, out.tracks, &staged.project.media)?;
        staged.project.tracks = tracks;
        new_selected = sel;
        staged.project.sort_events();
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

    let mut selection_changed = false;
    let new_sel_track = selected_tracks.first().copied();
    if new_sel_track != staged.sel_track {
        staged.sel_track = new_sel_track;
        selection_changed = true;
    }
    if selected_tracks.len() > 1 {
        let n = selected_tracks.first().map_or(0, |i| i + 1);
        messages_out.push(format!(
            "O Neoveg seleciona uma trilha por vez; ficou selecionada a trilha {n}."
        ));
    }
    let existing: std::collections::HashSet<u64> = staged
        .project
        .tracks
        .iter()
        .flat_map(|t| t.events.iter().map(|e| e.id))
        .collect();
    let new_sel_events: std::collections::HashSet<u64> = out
        .selected_events
        .iter()
        .copied()
        .filter(|id| existing.contains(id))
        .chain(new_selected)
        .collect();
    if new_sel_events != staged.sel_events {
        staged.sel_events = new_sel_events;
        selection_changed = true;
    }
    if let (Some(s), Some(l)) = (out.sel_start_ticks, out.sel_length_ticks) {
        let s = script_ticks(s, "início da seleção")?;
        let l = script_ticks(l, "duração da seleção")?;
        let end = Ticks::from_raw(
            s.0.checked_add(l.0)
                .ok_or_else(|| bad("fim da seleção fora do limite"))?,
        );
        // como na linha do tempo: a seleção de tempo e a região de loop andam juntas
        if l > Ticks::ZERO {
            staged.time_sel = Some((s, end));
            staged.project.loop_region = Some((s, end));
        } else {
            staged.time_sel = None;
        }
        selection_changed = true;
    }

    let cursor_changed = if let Some(t) = out.cursor_ticks {
        staged.cursor = script_ticks(t, "cursor")?.clamp_safe(Ticks::ZERO, Ticks::MAX);
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
    } else if cursor_changed || selection_changed {
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
    // mídia importada pelo script (AddMedia) fica aqui até o fim da execução
    let probe = ProbeRun::start(seq);
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
        let payload = build_snapshot(session, &result_path, None, probe.token())?;
        last_exit_code = dotnet_host::invoke_component(
            &env,
            &dll_path,
            "ScriptPortal.Vegas.NeovegBridge, NeovegScript",
            "RunEntry",
            &payload,
        )?;
    } else if env.has_sdk {
        let loader_dll = ensure_bridge_dll(&env)?;
        let bridge_payload =
            build_snapshot(session, &result_path, Some(script_path), probe.token())?;
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

        let payload = build_snapshot(session, &result_path, None, probe.token())?;
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

    apply_script_output(parsed, probe.take(), session, host, messages_out)
}
