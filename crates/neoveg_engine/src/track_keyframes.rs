//! Quadros-chave de trilha mostrados na linha do tempo.
//!
//! Comportamento tirado do manual do VEGAS Pro 21 (seção "Expanding or
//! collapsing track keyframes", p. 389): os quadros-chave de efeitos de vídeo
//! da trilha e de Track Motion aparecem abaixo da trilha de vídeo; podem ser
//! arrastados, e o menu de contexto adiciona, apaga e muda a interpolação.
//! Quadros-chave de evento não aparecem na linha do tempo.
//!
//! Só funções puras sobre [`Track`]; a UI desenha e chama estas funções. O
//! tempo dos quadros-chave de trilha é o da linha do tempo (o render avalia
//! `track.motion` e os efeitos da trilha no tempo do projeto).
//!
//! Nenhuma edição muda a imagem em silêncio (§0): adicionar grava o valor
//! que já vale naquele tempo; apagar o último quadro-chave de um parâmetro
//! guarda o valor dele como valor fixo; Track Motion nunca fica sem
//! quadro-chave.

use crate::model::{Curve, FxKind, Id, Ticks, Track, TrackKind};

/// Linha de quadros-chave de uma trilha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KfRowKind {
    /// Track Motion da trilha.
    Motion,
    /// Efeito da trilha (id da instância).
    Fx(Id),
}

#[derive(Clone, Debug, PartialEq)]
pub struct KfRow {
    pub kind: KfRowKind,
    pub label: String,
    /// Tempos dos quadros-chave, em ordem, já agrupados por tolerância.
    pub times: Vec<Ticks>,
    /// A linha guarda curva de interpolação por quadro-chave (só Track Motion).
    pub has_curve: bool,
}

/// Linhas de quadros-chave de uma trilha: Track Motion (se estiver em uso) e
/// uma linha por efeito de vídeo da trilha, mesmo sem quadros-chave, para
/// o primeiro poder ser criado pela linha do tempo. Trilhas de áudio não têm
/// linhas.
pub fn track_rows_in(track: &Track, tol: Ticks, lang: neoveg_common::Language) -> Vec<KfRow> {
    if track.kind != TrackKind::Video {
        return vec![];
    }
    let mut rows = vec![];
    if !track.motion.is_identity() || track.motion.keys.len() > 1 {
        rows.push(KfRow {
            kind: KfRowKind::Motion,
            label: neoveg_common::tr(lang, "Movimento da trilha").into(),
            times: group_times(track.motion.keys.iter().map(|k| k.t), tol),
            has_curve: true,
        });
    }
    for fx in track.fx.iter().filter(|f| !f.kind.is_audio()) {
        let label = match (&fx.ofx, fx.kind) {
            (Some(o), FxKind::Ofx) if !o.label.is_empty() => o.label.clone(),
            _ => fx.kind.name_in(lang).to_string(),
        };
        let anim = fx.params.iter().flat_map(|p| p.keys.iter().map(|k| k.0));
        let ofx = fx
            .ofx
            .iter()
            .flat_map(|o| o.params.iter())
            .flat_map(|p| p.keys.iter().map(|k| k.0));
        rows.push(KfRow {
            kind: KfRowKind::Fx(fx.id),
            label,
            times: group_times(anim.chain(ofx), tol),
            has_curve: true,
        });
    }
    rows
}

pub fn track_rows(track: &Track, tol: Ticks) -> Vec<KfRow> {
    track_rows_in(track, tol, neoveg_common::Language::PtBr)
}

/// Ordena e agrupa tempos a até `tol` uns dos outros (fica o primeiro).
pub fn group_times(times: impl Iterator<Item = Ticks>, tol: Ticks) -> Vec<Ticks> {
    let mut v: Vec<Ticks> = times.collect();
    v.sort();
    let mut out: Vec<Ticks> = Vec::with_capacity(v.len());
    for t in v {
        match out.last() {
            Some(last) if t.abs_diff(*last) <= tol => {}
            _ => out.push(t),
        }
    }
    out
}

/// Adiciona um quadro-chave em `t` com o valor que já vale ali. Devolve
/// `false` se a linha não existe na trilha.
pub fn add_key(track: &mut Track, row: &KfRowKind, t: Ticks, tol: Ticks) -> bool {
    let t = t.max(Ticks::ZERO);
    match row {
        KfRowKind::Motion => {
            let v = track.motion.eval(t);
            track.motion.set(t, v, tol);
            true
        }
        KfRowKind::Fx(id) => {
            let Some(fx) = track.fx.iter_mut().find(|f| f.id == *id) else {
                return false;
            };
            for p in &mut fx.params {
                let v = p.eval(t);
                insert_key(&mut p.keys, t, v, tol);
            }
            if let Some(o) = &mut fx.ofx {
                for p in &mut o.params {
                    let v = p.values_at(t);
                    insert_key(&mut p.keys, t, v, tol);
                }
            }
            true
        }
    }
}

fn insert_key<V>(keys: &mut Vec<(Ticks, V)>, t: Ticks, v: V, tol: Ticks) {
    if let Some(k) = keys.iter_mut().find(|k| k.0.abs_diff(t) <= tol) {
        k.1 = v;
        return;
    }
    let at = keys.partition_point(|k| k.0 < t);
    keys.insert(at, (t, v));
}

/// Apaga o quadro-chave em `t`. Track Motion nunca fica sem quadro-chave;
/// um parâmetro que perde o último quadro-chave passa a valer o valor dele.
/// Devolve `false` se nada foi apagado.
pub fn delete_key(track: &mut Track, row: &KfRowKind, t: Ticks, tol: Ticks) -> bool {
    match row {
        KfRowKind::Motion => {
            let keys = &mut track.motion.keys;
            if keys.len() <= 1 {
                return false;
            }
            let before = keys.len();
            keys.retain(|k| k.t.abs_diff(t) > tol);
            if keys.is_empty() {
                // não acontece com len > 1 e tempos distintos; por garantia
                return false;
            }
            keys.len() != before
        }
        KfRowKind::Fx(id) => {
            let Some(fx) = track.fx.iter_mut().find(|f| f.id == *id) else {
                return false;
            };
            let mut removed = false;
            for p in &mut fx.params {
                if let Some(last) = p.remove_key_near(t, tol) {
                    removed = true;
                    if p.keys.is_empty() {
                        p.value = last;
                    }
                }
            }
            if let Some(o) = &mut fx.ofx {
                for p in &mut o.params {
                    if let Some(last) = remove_near(&mut p.keys, t, tol) {
                        removed = true;
                        if p.keys.is_empty() {
                            p.values = last;
                        }
                    }
                }
            }
            removed
        }
    }
}

/// Remove os quadros-chave a até `tol` de `t`; devolve o valor do último removido.
fn remove_near<V>(keys: &mut Vec<(Ticks, V)>, t: Ticks, tol: Ticks) -> Option<V> {
    let mut last = None;
    let mut i = 0;
    while i < keys.len() {
        if keys.get(i).is_some_and(|k| k.0.abs_diff(t) <= tol) {
            last = Some(keys.remove(i).1);
        } else {
            i += 1;
        }
    }
    last
}

/// Move o quadro-chave que está em `from` para perto de `to`, sem passar
/// dos vizinhos da linha (mantém a ordem e a separação). Devolve o tempo
/// final, ou `None` se não há quadro-chave em `from`.
pub fn move_key(
    track: &mut Track,
    row: &KfRowKind,
    from: Ticks,
    to: Ticks,
    tol: Ticks,
) -> Option<Ticks> {
    let rows = track_rows(track, tol);
    let times = rows.iter().find(|r| r.kind == *row)?.times.clone();
    let idx = times.iter().position(|t| t.abs_diff(from) <= tol)?;
    let gap = tol + Ticks::from_raw(1);
    let lo = idx
        .checked_sub(1)
        .and_then(|i| times.get(i))
        .map_or(Ticks::ZERO, |p| *p + gap);
    let hi = times.get(idx + 1).map(|n| *n - gap);
    let mut target = to.max(lo);
    if let Some(hi) = hi {
        if hi < lo {
            // vizinhos colados: o quadro-chave fica onde está
            return times.get(idx).copied();
        }
        target = target.min(hi);
    }
    let anchor = times.get(idx).copied()?;
    match row {
        KfRowKind::Motion => {
            for k in &mut track.motion.keys {
                if k.t.abs_diff(anchor) <= tol {
                    k.t = target;
                }
            }
            track.motion.keys.sort_by_key(|k| k.t);
        }
        KfRowKind::Fx(id) => {
            let fx = track.fx.iter_mut().find(|f| f.id == *id)?;
            for p in &mut fx.params {
                p.retime(anchor, target, tol);
            }
            if let Some(o) = &mut fx.ofx {
                for p in &mut o.params {
                    retime(&mut p.keys, anchor, target, tol);
                }
            }
        }
    }
    Some(target)
}

fn retime<V>(keys: &mut [(Ticks, V)], anchor: Ticks, target: Ticks, tol: Ticks) {
    for k in keys.iter_mut() {
        if k.0.abs_diff(anchor) <= tol {
            k.0 = target;
        }
    }
    keys.sort_by_key(|k| k.0);
}

/// Muda a curva de interpolação do quadro-chave de Track Motion em `t` (a
/// curva vale do quadro-chave até o próximo). Devolve `false` se não há
/// quadro-chave ali.
pub fn set_motion_curve(track: &mut Track, t: Ticks, curve: Curve, tol: Ticks) -> bool {
    let mut found = false;
    for k in &mut track.motion.keys {
        if k.t.abs_diff(t) <= tol {
            k.curve = curve;
            found = true;
        }
    }
    found
}

/// Curva do quadro-chave de Track Motion em `t`, se houver.
pub fn motion_curve(track: &Track, t: Ticks, tol: Ticks) -> Option<Curve> {
    track
        .motion
        .keys
        .iter()
        .find(|k| k.t.abs_diff(t) <= tol)
        .map(|k| k.curve)
}

/// Muda a curva de interpolação do quadro-chave em `t` para o efeito da trilha `fx_id`.
pub fn set_fx_curve(track: &mut Track, fx_id: Id, t: Ticks, curve: Curve, tol: Ticks) -> bool {
    let Some(fx) = track.fx.iter_mut().find(|f| f.id == fx_id) else {
        return false;
    };
    let mut found = false;
    for p in &mut fx.params {
        if p.set_curve(t, curve, tol) {
            found = true;
        }
    }
    found
}

/// Curva do quadro-chave do efeito da trilha `fx_id` em `t`, se houver.
pub fn fx_curve(track: &Track, fx_id: Id, t: Ticks, tol: Ticks) -> Option<Curve> {
    let fx = track.fx.iter().find(|f| f.id == fx_id)?;
    for p in &fx.params {
        if let Some(c) = p.curve_at(t, tol) {
            return Some(c);
        }
    }
    None
}

/// Muda a curva de interpolação de qualquer linha de quadros-chave (Motion ou Fx).
pub fn set_key_curve(
    track: &mut Track,
    row: &KfRowKind,
    t: Ticks,
    curve: Curve,
    tol: Ticks,
) -> bool {
    match row {
        KfRowKind::Motion => set_motion_curve(track, t, curve, tol),
        KfRowKind::Fx(id) => set_fx_curve(track, *id, t, curve, tol),
    }
}

/// Curva de interpolação de qualquer linha de quadros-chave (Motion ou Fx).
pub fn key_curve(track: &Track, row: &KfRowKind, t: Ticks, tol: Ticks) -> Option<Curve> {
    match row {
        KfRowKind::Motion => motion_curve(track, t, tol),
        KfRowKind::Fx(id) => fx_curve(track, *id, t, tol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FxInstance, MotionKey, OfxFxRef, OfxParamValue};

    const TOL: Ticks = Ticks::from_raw(1000);

    fn video_track() -> Track {
        Track::new(TrackKind::Video, "V")
    }

    fn secs(s: i64) -> Ticks {
        Ticks::from_secs_i64(s)
    }

    fn fx_with_keys(keys: &[(i64, f32)]) -> FxInstance {
        let mut fx = FxInstance::new(FxKind::BrightnessContrast);
        if let Some(p) = fx.params.first_mut() {
            p.keys = keys.iter().map(|(s, v)| (secs(*s), *v)).collect();
        }
        fx
    }

    #[test]
    fn audio_tracks_have_no_rows_and_unused_motion_is_hidden() {
        let mut a = Track::new(TrackKind::Audio, "A");
        a.fx.push(FxInstance::new(FxKind::BrightnessContrast));
        assert!(track_rows(&a, TOL).is_empty());
        let v = video_track();
        assert!(track_rows(&v, TOL).is_empty(), "motion padrão não aparece");
    }

    #[test]
    fn rows_list_motion_and_every_video_fx_with_grouped_times() {
        let mut t = video_track();
        t.motion.set(secs(2), MotionKey::default(), TOL);
        let mut fx = fx_with_keys(&[(1, 0.1), (3, 0.3)]);
        // um segundo parâmetro com quadro-chave quase no mesmo tempo
        if let Some(p) = fx.params.get_mut(1) {
            p.keys = vec![(secs(1) + Ticks::from_raw(10), 0.5)];
        }
        t.fx.push(fx);
        t.fx.push(FxInstance::new(FxKind::BrightnessContrast));
        let rows = track_rows(&t, TOL);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, KfRowKind::Motion);
        assert_eq!(rows[0].times, vec![Ticks::ZERO, secs(2)]);
        assert_eq!(rows[1].times, vec![secs(1), secs(3)]);
        assert!(
            rows[2].times.is_empty(),
            "efeito sem quadro-chave tem linha vazia"
        );
    }

    #[test]
    fn add_keeps_the_current_value() {
        let mut t = video_track();
        t.fx.push(fx_with_keys(&[(0, 0.0), (4, 1.0)]));
        let id = t.fx[0].id;
        let before = t.fx[0].params[0].eval(secs(1));
        assert!(add_key(&mut t, &KfRowKind::Fx(id), secs(2), TOL));
        let p = &t.fx[0].params[0];
        assert_eq!(p.keys.len(), 3);
        // interpolação linear: a curva não muda em nenhum ponto amostrado
        for s in 0..=4 {
            let want = s as f32 / 4.0;
            assert!((p.eval(secs(s)) - want).abs() < 1e-5, "t={s}");
        }
        assert!((p.eval(secs(1)) - before).abs() < 1e-6);
        // parâmetro sem quadro-chave ganha um com o valor fixo dele
        let fixed = t.fx[0].params[1].value;
        assert_eq!(t.fx[0].params[1].keys, vec![(secs(2), fixed)]);
    }

    #[test]
    fn deleting_the_last_key_keeps_its_value() {
        let mut t = video_track();
        t.fx.push(fx_with_keys(&[(1, 0.7)]));
        let id = t.fx[0].id;
        assert!(delete_key(&mut t, &KfRowKind::Fx(id), secs(1), TOL));
        let p = &t.fx[0].params[0];
        assert!(p.keys.is_empty());
        assert!((p.value - 0.7).abs() < 1e-6);
        assert!(!delete_key(&mut t, &KfRowKind::Fx(id), secs(1), TOL));
    }

    #[test]
    fn motion_never_loses_its_last_key() {
        let mut t = video_track();
        assert!(!delete_key(&mut t, &KfRowKind::Motion, Ticks::ZERO, TOL));
        t.motion.set(secs(1), MotionKey::default(), TOL);
        assert!(delete_key(&mut t, &KfRowKind::Motion, secs(1), TOL));
        assert_eq!(t.motion.keys.len(), 1);
    }

    #[test]
    fn move_stops_before_the_neighbours() {
        let mut t = video_track();
        t.fx.push(fx_with_keys(&[(1, 0.1), (3, 0.3), (5, 0.5)]));
        let id = t.fx[0].id;
        let row = KfRowKind::Fx(id);
        let got = move_key(&mut t, &row, secs(3), secs(9), TOL).expect("move");
        assert!(got < secs(5) && got > secs(3));
        let got = move_key(&mut t, &row, got, Ticks::ZERO, TOL).expect("move");
        assert!(got > secs(1));
        let times: Vec<Ticks> = t.fx[0].params[0].keys.iter().map(|k| k.0).collect();
        assert!(times.windows(2).all(|w| w[0] < w[1]), "ordem mantida");
        assert_eq!(
            move_key(&mut t, &row, secs(2) + Ticks::from_raw(7), secs(4), TOL),
            None
        );
    }

    #[test]
    fn ofx_keys_are_listed_added_and_deleted() {
        let mut t = video_track();
        let mut fx = FxInstance::new(FxKind::Ofx);
        fx.ofx = Some(OfxFxRef {
            binary: "x.ofx".into(),
            plugin_id: "p".into(),
            label: "Brilho OFX".into(),
            params: vec![OfxParamValue {
                name: "amount".into(),
                values: vec![0.0],
                keys: vec![(secs(1), vec![0.2]), (secs(3), vec![0.6])],
                hold: false,
                integer: false,
            }],
        });
        let id = fx.id;
        t.fx.push(fx);
        let rows = track_rows(&t, TOL);
        assert_eq!(rows[0].label, "Brilho OFX");
        assert_eq!(rows[0].times, vec![secs(1), secs(3)]);
        assert!(add_key(&mut t, &KfRowKind::Fx(id), secs(2), TOL));
        let p = &t.fx[0].ofx.as_ref().expect("ofx").params[0];
        assert_eq!(p.keys.len(), 3);
        assert!((p.values_at(secs(2))[0] - 0.4).abs() < 1e-9);
        assert!(delete_key(&mut t, &KfRowKind::Fx(id), secs(1), TOL));
        assert!(delete_key(&mut t, &KfRowKind::Fx(id), secs(2), TOL));
        assert!(delete_key(&mut t, &KfRowKind::Fx(id), secs(3), TOL));
        let p = &t.fx[0].ofx.as_ref().expect("ofx").params[0];
        assert!(p.keys.is_empty());
        assert_eq!(p.values, vec![0.6]);
    }

    #[test]
    fn motion_curve_is_set_on_the_key() {
        let mut t = video_track();
        t.motion.set(secs(2), MotionKey::default(), TOL);
        assert!(set_motion_curve(&mut t, secs(2), Curve::Hold, TOL));
        assert_eq!(motion_curve(&t, secs(2), TOL), Some(Curve::Hold));
        assert!(!set_motion_curve(&mut t, secs(9), Curve::Hold, TOL));
    }

    #[test]
    fn fx_curve_is_set_on_the_key_and_evaluates_curved() {
        let mut t = video_track();
        let fx = fx_with_keys(&[(0, 0.0), (4, 1.0)]);
        let id = fx.id;
        t.fx.push(fx);

        let rows = track_rows(&t, TOL);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].has_curve, "linhas de efeito agora suportam curva");

        assert!(set_fx_curve(&mut t, id, secs(0), Curve::Fast, TOL));
        assert_eq!(fx_curve(&t, id, secs(0), TOL), Some(Curve::Fast));

        // Curva Rápida (Fast) em x=0.5 resulta em 1 - (1 - 0.5)^3 = 0.875
        let mid = t.fx[0].params[0].eval(secs(2));
        assert!((mid - 0.875).abs() < 1e-4, "valor intermediário={mid}");

        // set_key_curve e key_curve unificados
        assert!(set_key_curve(
            &mut t,
            &KfRowKind::Fx(id),
            secs(0),
            Curve::Hold,
            TOL
        ));
        assert_eq!(
            key_curve(&t, &KfRowKind::Fx(id), secs(0), TOL),
            Some(Curve::Hold)
        );
        assert_eq!(t.fx[0].params[0].eval(secs(2)), 0.0);
    }
}
