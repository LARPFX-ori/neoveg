//! Parser e sintetizador auxiliar de arquivos Standard MIDI File (`.mid`/`.midi`)
//! implementado em sala limpa a partir da especificação pública SMF 1.0 (MIDI
//! Manufacturers Association, 1996).
//!
//! Alimenta instrumentos **VST3** (`IEventList`, `kNoteOnEvent`/`kNoteOffEvent`)
//! com precisão de amostra no mixer e oferece síntese polifônica de prévia para
//! forma de onda e reprodução imediata quando nenhum instrumento VST3 está
//! carregado na cadeia.

use crate::time::{Ticks, TICKS_PER_SEC};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

/// Tamanho máximo aceito de um arquivo `.mid` (16 MiB, §0.4).
const MAX_MIDI_BYTES: u64 = 16 * 1024 * 1024;
/// Máximo de trilhas SMF aceitas (§0.4).
const MAX_TRACKS: usize = 256;
/// Máximo de eventos lidos por trilha SMF (§0.4).
const MAX_EVENTS_PER_TRACK: usize = 200_000;
/// Máximo de notas guardadas num clipe MIDI (§0.4).
const MAX_NOTES: usize = 100_000;
/// Duração máxima de um clipe MIDI em segundos (2 horas, §0.4).
const MAX_DURATION_SECS: i64 = 7200;
/// Limite de duração pré-sintetizada para prévia/forma de onda (10 min, §0.4).
const MAX_FALLBACK_SECS: i64 = 600;
/// Tempo padrão MIDI: 120 BPM = 500 000 µs por semínima.
const DEFAULT_US_PER_QUARTER: u64 = 500_000;

/// Uma nota MIDI resolvida em tempo exato (`Ticks`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNote {
    pub start: Ticks,
    pub duration: Ticks,
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
}

/// Evento MIDI pronto para envio ao host VST3 (`IEventList`), com deslocamento
/// em amostras relativo ao início do bloco atual.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vst3MidiEvent {
    pub sample_offset: i32,
    pub is_note_on: bool,
    pub channel: u8,
    pub pitch: u8,
    pub velocity: f32,
    pub note_id: i32,
}

/// Conteúdo decodificado de um arquivo Standard MIDI File (`.mid`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiClip {
    pub tpqn: u16,
    pub notes: Vec<MidiNote>,
    pub duration: Ticks,
}

impl MidiClip {
    /// Extrai os eventos `NoteOn` e `NoteOff` que caem dentro da janela de tempo
    /// do clipe `[win_start, win_end)`, mapeando cada instante para o
    /// `sample_offset` em `0..frames`.
    pub fn events_in_window(
        &self,
        win_start: Ticks,
        win_end: Ticks,
        frames: usize,
        out: &mut Vec<Vst3MidiEvent>,
    ) {
        if frames == 0 || win_end <= win_start {
            return;
        }
        let span = (win_end - win_start).0;
        if span <= 0 {
            return;
        }
        let max_off = (frames.saturating_sub(1)).min(i32::MAX as usize) as i32;
        let to_offset = |t: Ticks| -> i32 {
            let rel = (t - win_start).0.clamp(0, span);
            let off = ((rel as i128).saturating_mul(frames as i128) / (span as i128)) as i64;
            (off as i32).clamp(0, max_off)
        };
        for (idx, n) in self.notes.iter().enumerate() {
            let note_end = Ticks::from_raw(n.start.0.saturating_add(n.duration.0));
            if note_end < win_start {
                continue;
            }
            if n.start >= win_end {
                break;
            }
            let note_id = (idx % (i32::MAX as usize)) as i32 + 1;
            let vel = (f32::from(n.velocity) / 127.0).clamp(0.0, 1.0);
            if n.start >= win_start && n.start < win_end {
                out.push(Vst3MidiEvent {
                    sample_offset: to_offset(n.start),
                    is_note_on: true,
                    channel: n.channel & 0x0F,
                    pitch: n.pitch & 0x7F,
                    velocity: vel,
                    note_id,
                });
            }
            if note_end >= win_start && note_end < win_end {
                out.push(Vst3MidiEvent {
                    sample_offset: to_offset(note_end),
                    is_note_on: false,
                    channel: n.channel & 0x0F,
                    pitch: n.pitch & 0x7F,
                    velocity: 0.0,
                    note_id,
                });
            }
        }
        out.sort_by_key(|e| (e.sample_offset, e.is_note_on));
    }

    /// Sintetiza uma prévia estéreo polifônica leve (senoide + harmônico suave
    /// com envelope ADSR) para desenhar a forma de onda e reproduzir o clipe
    /// quando nenhum instrumento VST3 está aplicado.
    pub fn render_fallback_stereo(&self, sr: u32) -> Vec<f32> {
        let sr = sr.clamp(8000, 192_000);
        let dur = self.duration.clamp(
            Ticks::from_millis_i64(250),
            Ticks::from_secs_i64(MAX_FALLBACK_SECS),
        );
        let total_frames = dur.to_samples_round(sr).max(1) as usize;
        let max_frames = (sr as usize).saturating_mul(MAX_FALLBACK_SECS as usize);
        let frames = total_frames.min(max_frames);
        let mut out = vec![0.0f32; frames.saturating_mul(2)];
        let sr_f = sr as f64;
        let attack = (0.008 * sr_f) as usize;
        let release = (0.04 * sr_f) as usize;

        for n in self.notes.iter().take(MAX_NOTES) {
            let start_f = n.start.to_samples_round(sr).max(0) as usize;
            if start_f >= frames {
                continue;
            }
            let len_f = n.duration.to_samples_round(sr).max(1) as usize;
            let end_f = start_f
                .saturating_add(len_f)
                .saturating_add(release)
                .min(frames);
            let freq = 440.0 * 2.0_f64.powf((f64::from(n.pitch) - 69.0) / 12.0);
            let amp = (f32::from(n.velocity) / 127.0) * 0.14;
            let phase_inc = std::f64::consts::TAU * freq / sr_f;
            let mut phase = 0.0f64;
            for i in start_f..end_f {
                let rel = i - start_f;
                let env = if rel < attack && attack > 0 {
                    rel as f32 / attack as f32
                } else if rel >= len_f {
                    let tail = rel - len_f;
                    (1.0 - tail as f32 / release.max(1) as f32).max(0.0)
                } else {
                    1.0
                };
                let s = ((phase.sin() * 0.78 + (phase * 2.0).sin() * 0.22) as f32) * amp * env;
                let idx = i * 2;
                out[idx] += s;
                out[idx + 1] += s;
                phase += phase_inc;
            }
        }
        for s in &mut out {
            *s = s.clamp(-0.98, 0.98);
        }
        out
    }
}

/// Lê uma quantidade de comprimento variável (VLQ) SMF (até 4 bytes).
fn read_vlq(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut val: u32 = 0;
    for _ in 0..4 {
        let &b = data.get(*pos)?;
        *pos += 1;
        val = (val << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            return Some(val);
        }
    }
    None
}

/// Converte um instante em ticks MIDI para `Ticks` do Neoveg usando o mapa de
/// tempo ordenado `(midi_tick, us_per_quarter)`.
fn midi_tick_to_ticks(tick: u64, tpqn: u16, tempos: &[(u64, u64)]) -> Ticks {
    let tpqn = u64::from(tpqn.max(1));
    let mut prev_tick = 0u64;
    let mut cur_us = DEFAULT_US_PER_QUARTER;
    let mut acc_ticks: i128 = 0;
    for &(t_pos, us) in tempos {
        if t_pos >= tick {
            break;
        }
        let dt = t_pos.saturating_sub(prev_tick) as i128;
        acc_ticks = acc_ticks.saturating_add(
            dt.saturating_mul(cur_us as i128)
                .saturating_mul(TICKS_PER_SEC as i128)
                / (tpqn as i128 * 1_000_000),
        );
        prev_tick = t_pos;
        cur_us = us.clamp(10_000, 10_000_000);
    }
    let rem = tick.saturating_sub(prev_tick) as i128;
    acc_ticks = acc_ticks.saturating_add(
        rem.saturating_mul(cur_us as i128)
            .saturating_mul(TICKS_PER_SEC as i128)
            / (tpqn as i128 * 1_000_000),
    );
    let max_raw = Ticks::from_secs_i64(MAX_DURATION_SECS).0 as i128;
    Ticks::from_raw(acc_ticks.clamp(0, max_raw) as i64)
}

/// Faz o parse de um arquivo Standard MIDI File (SMF 0/1/2) tratando toda
/// entrada como hostil (§0.4).
pub fn parse_smf(bytes: &[u8]) -> Result<MidiClip, String> {
    if bytes.len() < 14 {
        return Err("arquivo MIDI curto demais".into());
    }
    if (bytes.len() as u64) > MAX_MIDI_BYTES {
        return Err("arquivo MIDI excede o limite de 16 MiB".into());
    }
    if bytes.get(0..4) != Some(b"MThd") {
        return Err("cabeçalho MThd ausente (não é um arquivo MIDI SMF)".into());
    }
    let hdr_len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if hdr_len < 6 || bytes.len() < 8usize.saturating_add(hdr_len) {
        return Err("cabeçalho MThd inválido".into());
    }
    let _format = u16::from_be_bytes([bytes[8], bytes[9]]);
    let ntracks = (u16::from_be_bytes([bytes[10], bytes[11]]) as usize).min(MAX_TRACKS);
    let division_raw = u16::from_be_bytes([bytes[12], bytes[13]]);
    let tpqn = if division_raw & 0x8000 != 0 {
        // SMPTE timecode division: aproxima para ticks por semínima razoável
        let fps = (-(division_raw as i16 >> 8)).clamp(24, 30) as u16;
        let sub = (division_raw & 0x00FF).max(1);
        fps.saturating_mul(sub).max(1)
    } else if division_raw == 0 {
        return Err("divisão MIDI (TPQN) nula".into());
    } else {
        division_raw
    };

    let mut pos = 8usize.saturating_add(hdr_len);
    let mut tempos: Vec<(u64, u64)> = vec![(0, DEFAULT_US_PER_QUARTER)];
    // (midi_start, midi_end, channel, pitch, velocity)
    let mut raw_notes: Vec<(u64, u64, u8, u8, u8)> = Vec::new();
    let mut max_midi_tick: u64 = 0;

    for _ in 0..ntracks {
        if pos.saturating_add(8) > bytes.len() {
            break;
        }
        let tag = bytes.get(pos..pos + 4);
        let chunk_len = u32::from_be_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        pos = pos.saturating_add(8);
        let chunk_end = pos.saturating_add(chunk_len).min(bytes.len());
        let track = bytes.get(pos..chunk_end).unwrap_or(&[]);
        pos = chunk_end;
        if tag != Some(b"MTrk") {
            continue;
        }

        let mut tp = 0usize;
        let mut abs_tick: u64 = 0;
        let mut running_status: u8 = 0;
        // notas ativas nesta trilha: (channel, pitch) -> (start_tick, velocity)
        let mut active: Vec<(u8, u8, u64, u8)> = Vec::new();
        let mut events = 0usize;

        while tp < track.len() && events < MAX_EVENTS_PER_TRACK {
            events += 1;
            let Some(delta) = read_vlq(track, &mut tp) else {
                break;
            };
            abs_tick = abs_tick.saturating_add(u64::from(delta));
            max_midi_tick = max_midi_tick.max(abs_tick);
            let Some(&first) = track.get(tp) else {
                break;
            };
            let status = if first & 0x80 != 0 {
                tp += 1;
                if first < 0xF0 {
                    running_status = first;
                }
                first
            } else if running_status >= 0x80 {
                running_status
            } else {
                break;
            };

            match status {
                0xFF => {
                    // Meta-evento
                    let Some(&meta_type) = track.get(tp) else {
                        break;
                    };
                    tp += 1;
                    let Some(len) = read_vlq(track, &mut tp) else {
                        break;
                    };
                    let len = len as usize;
                    let data_end = tp.saturating_add(len).min(track.len());
                    if meta_type == 0x51 && data_end.saturating_sub(tp) == 3 {
                        let us = (u64::from(track[tp]) << 16)
                            | (u64::from(track[tp + 1]) << 8)
                            | u64::from(track[tp + 2]);
                        if us > 0 {
                            tempos.push((abs_tick, us));
                        }
                    }
                    tp = data_end;
                    if meta_type == 0x2F {
                        break;
                    }
                }
                0xF0 | 0xF7 => {
                    // SysEx
                    let Some(len) = read_vlq(track, &mut tp) else {
                        break;
                    };
                    tp = tp.saturating_add(len as usize).min(track.len());
                }
                0x80..=0xEF => {
                    let kind = status & 0xF0;
                    let ch = status & 0x0F;
                    let data_bytes = if kind == 0xC0 || kind == 0xD0 { 1 } else { 2 };
                    if tp.saturating_add(data_bytes) > track.len() {
                        break;
                    }
                    let d1 = track[tp] & 0x7F;
                    let d2 = if data_bytes == 2 {
                        track[tp + 1] & 0x7F
                    } else {
                        0
                    };
                    tp += data_bytes;
                    if kind == 0x90 && d2 > 0 {
                        if active.len() < 128 {
                            active.push((ch, d1, abs_tick, d2));
                        }
                    } else if kind == 0x80 || (kind == 0x90 && d2 == 0) {
                        if let Some(i) = active.iter().position(|&(c, p, _, _)| c == ch && p == d1)
                        {
                            let (_, _, st, vel) = active.swap_remove(i);
                            if raw_notes.len() < MAX_NOTES {
                                raw_notes.push((st, abs_tick.max(st + 1), ch, d1, vel));
                            }
                        }
                    }
                }
                _ => {
                    // Mensagem de sistema inesperada: encerra trilha com segurança
                    break;
                }
            }
        }
        // Fecha notas que ficaram abertas até o fim da trilha
        for (ch, pitch, st, vel) in active {
            if raw_notes.len() < MAX_NOTES {
                raw_notes.push((st, abs_tick.max(st + 1), ch, pitch, vel));
            }
        }
    }

    tempos.sort_by_key(|t| t.0);
    let mut notes: Vec<MidiNote> = raw_notes
        .into_iter()
        .map(|(st, en, channel, pitch, velocity)| {
            let start = midi_tick_to_ticks(st, tpqn, &tempos);
            let end = midi_tick_to_ticks(en, tpqn, &tempos);
            let duration = (end - start).max(Ticks::from_millis_i64(10));
            MidiNote {
                start,
                duration,
                channel,
                pitch,
                velocity,
            }
        })
        .collect();
    notes.sort_by_key(|n| (n.start, n.pitch));

    let notes_end = notes
        .iter()
        .map(|n| Ticks::from_raw(n.start.0.saturating_add(n.duration.0)))
        .max()
        .unwrap_or(Ticks::ZERO);
    let track_end = midi_tick_to_ticks(max_midi_tick, tpqn, &tempos);
    let duration = notes_end
        .max(track_end)
        .max(Ticks::from_secs_i64(1))
        .min(Ticks::from_secs_i64(MAX_DURATION_SECS));

    Ok(MidiClip {
        tpqn,
        notes,
        duration,
    })
}

/// Carrega um arquivo `.mid`/`.midi` do disco com limite de tamanho (§0.4).
pub fn load_smf_file(path: &Path) -> Result<MidiClip, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("erro ao abrir MIDI: {e}"))?;
    let mut bytes = Vec::new();
    f.take(MAX_MIDI_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("erro ao ler MIDI: {e}"))?;
    parse_smf(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_simple_smf(tpqn: u16, tempo_us: u32) -> Vec<u8> {
        let mut trk = Vec::new();
        // delta 0, meta tempo 0xFF 0x51 0x03 <3 bytes>
        trk.extend_from_slice(&[
            0x00,
            0xFF,
            0x51,
            0x03,
            ((tempo_us >> 16) & 0xFF) as u8,
            ((tempo_us >> 8) & 0xFF) as u8,
            (tempo_us & 0xFF) as u8,
        ]);
        // delta 0: Note On C4 (60), vel 100
        trk.extend_from_slice(&[0x00, 0x90, 60, 100]);
        // delta tpqn (1 beat = 0.5 s @ 120 BPM): Note Off C4 (via vel 0 running status)
        trk.extend_from_slice(&[0x83, 0x60, 60, 0]); // VLQ 480 = 0x83 0x60
                                                     // delta 0: Note On E4 (64), vel 90
        trk.extend_from_slice(&[0x00, 0x90, 64, 90]);
        // delta 480: Note Off E4
        trk.extend_from_slice(&[0x83, 0x60, 0x80, 64, 0]);
        // End of track
        trk.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

        let mut out = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // format 0
        out.extend_from_slice(&1u16.to_be_bytes()); // 1 track
        out.extend_from_slice(&tpqn.to_be_bytes());
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(trk.len() as u32).to_be_bytes());
        out.extend_from_slice(&trk);
        out
    }

    #[test]
    fn parse_smf_extrai_notas_janelas_e_sintetiza_previa() {
        let smf = build_simple_smf(480, 500_000);
        let clip = parse_smf(&smf).expect("parse SMF válido");
        assert_eq!(clip.tpqn, 480);
        assert_eq!(clip.notes.len(), 2);
        assert_eq!(clip.notes[0].pitch, 60);
        assert_eq!(clip.notes[0].start, Ticks::ZERO);
        assert_eq!(clip.notes[0].duration, Ticks::from_millis_i64(500));
        assert_eq!(clip.notes[1].pitch, 64);
        assert_eq!(clip.notes[1].start, Ticks::from_millis_i64(500));
        assert_eq!(clip.notes[1].duration, Ticks::from_millis_i64(500));

        // Janela [0.4 s, 0.6 s) deve capturar NoteOff de C4 e NoteOn de E4 exatamente no meio
        let mut evs = Vec::new();
        clip.events_in_window(
            Ticks::from_millis_i64(400),
            Ticks::from_millis_i64(600),
            1000,
            &mut evs,
        );
        assert_eq!(evs.len(), 2);
        assert!(!evs[0].is_note_on && evs[0].pitch == 60);
        assert!(evs[1].is_note_on && evs[1].pitch == 64);
        assert!((evs[0].sample_offset - 500).abs() <= 1);
        assert!((evs[1].sample_offset - 500).abs() <= 1);

        // Síntese fallback gera áudio não nulo e finito
        let pcm = clip.render_fallback_stereo(8000);
        assert!(!pcm.is_empty());
        assert!(pcm.iter().any(|&s| s.abs() > 0.01));
        assert!(pcm.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn parse_smf_entradas_hostis_e_fuzz_sem_panico() {
        assert!(parse_smf(b"").is_err());
        assert!(parse_smf(b"RIFF\0\0\0\0WAVEfmt ").is_err());
        // Divisão nula rejeitada
        let bad_div = build_simple_smf(0, 500_000);
        assert!(parse_smf(&bad_div).is_err());

        let valid = build_simple_smf(480, 500_000);
        for cut in 0..valid.len() {
            let slice = &valid[..cut];
            assert!(std::panic::catch_unwind(|| parse_smf(slice)).is_ok());
        }
        for i in 0..valid.len() {
            let mut mutated = valid.clone();
            mutated[i] ^= 0xFF;
            assert!(std::panic::catch_unwind(|| parse_smf(&mutated)).is_ok());
        }
    }
}
