//! Mixagem de áudio: eventos -> trilhas (FX, volume, pan, envelopes) ->
//! master bus (FX, volume).

use super::afx::{self, FxState};
use crate::media::AudioData;
use crate::model::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct Meters {
    /// Pico por trilha (L, R).
    pub tracks: HashMap<Id, [f32; 2]>,
    pub master: [f32; 2],
}

pub struct Mixer {
    pub sr: u32,
    states: HashMap<Id, FxState>,
    pub meters: Meters,
    track_buf: Vec<f32>,
    ev_buf: Vec<f32>,
    master_buf: Vec<f32>,
}

/// Fades efetivos de um evento de áudio: o maior entre o fade definido e a
/// sobreposição com o evento vizinho (crossfade automático).
fn effective_fades(track: &Track, idx: usize) -> (Fade, Fade) {
    let e = &track.events[idx];
    let mut fi = e.fade_in.clone();
    let mut fo = e.fade_out.clone();
    for (j, o) in track.events.iter().enumerate() {
        if j == idx || o.mute {
            continue;
        }
        if o.start < e.start && o.end() > e.start && o.end() < e.end() {
            let ov = o.end() - e.start;
            if ov > fi.len {
                fi.len = ov;
            }
        }
        if o.start > e.start && o.start < e.end() && o.end() > e.end() {
            let ov = e.end() - o.start;
            if ov > fo.len {
                fo.len = ov;
            }
        }
    }
    (fi, fo)
}

#[inline]
fn fade_gain(e: &Event, fi: &Fade, fo: &Fade, t: Ticks) -> f32 {
    let local = t - e.start;
    let mut g = 1.0;
    if fi.len > Ticks::ZERO && local < fi.len {
        g *= fi.curve.shape(local.fraction_in(Ticks::ZERO, fi.len));
    }
    let rem = e.end() - t;
    if fo.len > Ticks::ZERO && rem < fo.len {
        g *= fo.curve.shape(rem.fraction_in(Ticks::ZERO, fo.len));
    }
    g
}

impl Mixer {
    pub fn new(sr: u32) -> Self {
        Mixer {
            sr,
            states: HashMap::new(),
            meters: Meters::default(),
            track_buf: vec![],
            ev_buf: vec![],
            master_buf: vec![],
        }
    }

    /// Zera o estado dos efeitos (após salto de posição).
    pub fn reset(&mut self) {
        self.states.clear();
    }

    fn run_fx(&mut self, fx: &FxInstance, buf: &mut [f32], t: Ticks) {
        let sr = self.sr;
        let st = self
            .states
            .entry(fx.id)
            .or_insert_with(|| FxState::new(fx.kind, sr));
        afx::process(fx, st, sr, buf, t);
    }

    /// Mixa `out.len()/2` quadros a partir da amostra `start` (na taxa `sr`).
    pub fn process(
        &mut self,
        p: &Project,
        audio: &HashMap<Id, Arc<AudioData>>,
        start: i64,
        out: &mut [f32],
    ) {
        let n = out.len() / 2;
        let sr = self.sr;
        let step = Ticks::exact_step_for_sample_rate(sr);
        let sample_time = |idx: i64| -> Ticks {
            match step {
                Some(k) => Ticks(idx.saturating_mul(k)),
                None => Ticks::from_samples(idx, sr),
            }
        };
        let t0 = sample_time(start);
        let t1 = sample_time(start.saturating_add(n as i64));
        out.iter_mut().for_each(|s| *s = 0.0);
        self.master_buf.resize(n * 2, 0.0);
        self.master_buf.iter_mut().for_each(|s| *s = 0.0);
        let any_solo = p.any_solo(TrackKind::Audio);
        let mut track_buf = std::mem::take(&mut self.track_buf);
        let mut ev_buf = std::mem::take(&mut self.ev_buf);
        track_buf.resize(n * 2, 0.0);
        ev_buf.resize(n * 2, 0.0);

        for track in &p.tracks {
            if track.kind != TrackKind::Audio {
                continue;
            }
            if !track.is_audible(any_solo) {
                self.meters.tracks.insert(track.id, [0.0, 0.0]);
                continue;
            }
            track_buf.iter_mut().for_each(|s| *s = 0.0);
            let mut any = false;
            for (ei, e) in track.events.iter().enumerate() {
                if e.mute || e.end() <= t0 || e.start >= t1 {
                    continue;
                }
                let Some(m) = p.media(e.media_id) else {
                    continue;
                };
                let Some(a) = audio.get(&e.media_id) else {
                    continue;
                };
                let dur = m.duration.or_else(|| Some(a.duration()));
                let (fi, fo) = effective_fades(track, ei);
                let g = db_to_lin(e.gain_db);
                ev_buf.iter_mut().for_each(|s| *s = 0.0);
                for i in 0..n {
                    let t = sample_time(start.saturating_add(i as i64));
                    if !e.contains(t) {
                        continue;
                    }
                    let Some(mt) = e.media_time(t, dur) else {
                        continue;
                    };
                    let (l, r) = a.sample(mt);
                    let k = g * fade_gain(e, &fi, &fo, t);
                    ev_buf[i * 2] = l * k;
                    ev_buf[i * 2 + 1] = r * k;
                }
                for fx in &e.fx {
                    self.run_fx(fx, &mut ev_buf, t0 - e.start);
                }
                for (d, s) in track_buf.iter_mut().zip(ev_buf.iter()) {
                    *d += *s;
                }
                any = true;
            }
            let _ = any;
            for fx in &track.fx {
                self.run_fx(fx, &mut track_buf, t0);
            }
            // volume, envelope de volume e pan
            let vol = db_to_lin(track.volume_db);
            let venv = track.envelope(EnvKind::Volume);
            let penv = track.envelope(EnvKind::Pan);
            let mut peak = [0f32; 2];
            for i in 0..n {
                let t = sample_time(start.saturating_add(i as i64));
                let ve = venv.map(|e| e.eval(t)).unwrap_or(1.0);
                let pan = (track.pan + penv.map(|e| e.eval(t)).unwrap_or(0.0)).clamp(-1.0, 1.0);
                let gl = vol * ve * (1.0 - pan).min(1.0);
                let gr = vol * ve * (1.0 + pan).min(1.0);
                let l = track_buf[i * 2] * gl;
                let r = track_buf[i * 2 + 1] * gr;
                peak[0] = peak[0].max(l.abs());
                peak[1] = peak[1].max(r.abs());
                self.master_buf[i * 2] += l;
                self.master_buf[i * 2 + 1] += r;
            }
            self.meters.tracks.insert(track.id, peak);
        }
        let mut master = std::mem::take(&mut self.master_buf);
        for fx in &p.master.fx {
            self.run_fx(fx, &mut master, t0);
        }
        let mv = if p.master.mute {
            0.0
        } else {
            db_to_lin(p.master.volume_db)
        };
        let mut peak = [0f32; 2];
        for i in 0..n {
            let l = master[i * 2] * mv;
            let r = master[i * 2 + 1] * mv;
            peak[0] = peak[0].max(l.abs());
            peak[1] = peak[1].max(r.abs());
            out[i * 2] = l;
            out[i * 2 + 1] = r;
        }
        self.meters.master = peak;
        self.master_buf = master;
        self.track_buf = track_buf;
        self.ev_buf = ev_buf;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_mute_silencia_mixagem_e_zera_medidor() {
        let mut p = Project::new_default();
        let mid = new_id();
        let item = MediaItem {
            id: mid,
            name: "tom".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(2)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: "48k".into(),
            proxy_path: None,
        };
        let audio_track = p
            .tracks
            .iter_mut()
            .find(|t| t.kind == TrackKind::Audio)
            .expect("projeto padrão contém faixa de áudio");
        audio_track
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.media.push(item);
        let mut audio = HashMap::new();
        audio.insert(
            mid,
            Arc::new(AudioData::from_stereo(48000, vec![0.5; 48000 * 2])),
        );

        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 256 * 2];
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().any(|&v| v.abs() > 0.1));
        assert!(mixer.meters.master[0] > 0.1);

        p.master.mute = true;
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().all(|&v| v == 0.0));
        assert_eq!(mixer.meters.master, [0.0, 0.0]);
    }
}
