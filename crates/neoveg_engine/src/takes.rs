//! Takes: mídias alternativas de um evento, com uma ativa (manual do VEGAS
//! Pro 21, "Using takes as alternate versions of events", pp. 346–348).
//!
//! O take ativo fica nos campos de sempre do evento (`media_id`, `offset`,
//! `take_name`), para render e edição não mudarem; os outros ficam em
//! `Event::alt_takes`, e `Event::take_index` diz em que posição da lista
//! completa o ativo está. Estas funções são a única forma de mexer nessa
//! representação.

use crate::model::{Event, Id, Take, Ticks};
use std::collections::HashSet;

/// Máximo de takes por evento (§0.4).
pub const MAX_TAKES: usize = 64;

/// Quantos takes o evento tem (sempre pelo menos 1, o ativo).
pub fn take_count(e: &Event) -> usize {
    e.alt_takes.len() + 1
}

/// Posição do take ativo na lista completa.
pub fn active_index(e: &Event) -> usize {
    (e.take_index as usize).min(e.alt_takes.len())
}

/// O take ativo como `Take`.
fn active_take(e: &Event) -> Take {
    Take {
        media_id: e.media_id,
        offset: e.offset,
        name: e.take_name.clone(),
    }
}

/// Lista completa de takes, na ordem, com o ativo na posição dele.
pub fn takes(e: &Event) -> Vec<Take> {
    let mut v = e.alt_takes.clone();
    v.insert(active_index(e), active_take(e));
    v
}

/// Grava a lista completa de volta no evento, com o take `active` ativo.
fn store(e: &mut Event, mut list: Vec<Take>, active: usize) {
    if list.is_empty() {
        return;
    }
    let active = active.min(list.len() - 1);
    let a = list.remove(active);
    e.media_id = a.media_id;
    e.offset = a.offset;
    e.take_name = a.name;
    e.alt_takes = list;
    e.take_index = u32::try_from(active).unwrap_or(0);
}

/// Troca a lista inteira de takes, com `active` como o ativo (ponte de
/// scripts). Devolve `false`, sem mudar nada, se a lista está vazia, passa de
/// [`MAX_TAKES`] ou `active` não existe.
pub fn replace_takes(e: &mut Event, list: Vec<Take>, active: usize) -> bool {
    if list.is_empty() || list.len() > MAX_TAKES || active >= list.len() {
        return false;
    }
    store(e, list, active);
    true
}

/// Torna ativo o take `k`. Devolve `false` se `k` não existe ou já é o ativo.
pub fn set_active(e: &mut Event, k: usize) -> bool {
    if k >= take_count(e) || k == active_index(e) {
        return false;
    }
    let list = takes(e);
    store(e, list, k);
    true
}

/// Próximo take (T), voltando ao primeiro depois do último.
pub fn next_take(e: &mut Event) -> bool {
    let n = take_count(e);
    n > 1 && set_active(e, (active_index(e) + 1) % n)
}

/// Take anterior (Shift+T), indo ao último antes do primeiro.
pub fn prev_take(e: &mut Event) -> bool {
    let n = take_count(e);
    n > 1 && set_active(e, (active_index(e) + n - 1) % n)
}

/// Adiciona um take. Como em "Add As Takes" (p. 346), o novo vira o ativo
/// quando `make_active`, e o anterior continua como take. Devolve `false` no
/// limite de takes.
pub fn add_take(e: &mut Event, take: Take, make_active: bool) -> bool {
    if take_count(e) >= MAX_TAKES {
        return false;
    }
    let mut list = takes(e);
    let active = active_index(e);
    list.push(take);
    let new_active = if make_active { list.len() - 1 } else { active };
    store(e, list, new_active);
    true
}

/// Apaga o take `k`. Um evento nunca fica sem take: com um take só,
/// devolve `false`. Apagando o ativo, o seguinte (ou o anterior, se era o
/// último) vira o ativo.
pub fn delete_take(e: &mut Event, k: usize) -> bool {
    let n = take_count(e);
    if n <= 1 || k >= n {
        return false;
    }
    let active = active_index(e);
    let mut list = takes(e);
    list.remove(k);
    let new_active = if k < active {
        active - 1
    } else if k == active {
        active.min(list.len() - 1)
    } else {
        active
    };
    store(e, list, new_active);
    true
}

/// Apaga o take ativo ("Delete Active", p. 347).
pub fn delete_active(e: &mut Event) -> bool {
    let k = active_index(e);
    delete_take(e, k)
}

/// Desloca a mídia dos takes alternativos no slip com "Slip All Takes":
/// mesmo `delta` do take ativo, sem passar de zero; em evento em loop, volta
/// ao começo da mídia de cada take (`dur_of` dá a duração de cada mídia).
pub fn slip_alt_takes(e: &mut Event, delta: Ticks, dur_of: impl Fn(Id) -> Option<Ticks>) {
    let looped = e.looped;
    for t in &mut e.alt_takes {
        let mut o = t.offset + delta;
        if looped {
            if let Some(d) = dur_of(t.media_id).filter(|d| *d > Ticks::ZERO) {
                o = o.rem_euclid(d);
            }
        }
        t.offset = o.max(Ticks::ZERO);
    }
}

/// O evento usa a mídia `id` em algum take?
pub fn uses_media(e: &Event, id: Id) -> bool {
    e.media_id == id || e.alt_takes.iter().any(|t| t.media_id == id)
}

/// Todas as mídias usadas pelo evento (ativa e alternativas).
pub fn media_ids(e: &Event) -> impl Iterator<Item = Id> + '_ {
    std::iter::once(e.media_id).chain(e.alt_takes.iter().map(|t| t.media_id))
}

/// Tira do evento os takes da mídia `id`. Devolve `false` quando o take
/// ativo usa essa mídia e não há outro take para ficar no lugar (o evento
/// deve então ser apagado).
pub fn remove_media(e: &mut Event, id: Id) -> bool {
    let list: Vec<Take> = takes(e);
    let active = active_index(e);
    let keep: Vec<(usize, Take)> = list
        .into_iter()
        .enumerate()
        .filter(|(_, t)| t.media_id != id)
        .collect();
    if keep.is_empty() {
        return false;
    }
    let new_active = keep.iter().position(|(i, _)| *i == active).unwrap_or(0);
    store(e, keep.into_iter().map(|(_, t)| t).collect(), new_active);
    true
}

/// Substitui referências à mídia `old_id` por `new_id` (take ativo e takes
/// alternativos). Retorna `true` se o evento utilizava `old_id`.
pub fn replace_media_id(
    e: &mut Event,
    old_id: Id,
    new_id: Id,
    old_name: &str,
    new_name: &str,
) -> bool {
    let mut changed = false;
    if e.media_id == old_id {
        e.media_id = new_id;
        if e.name == old_name {
            e.name = new_name.to_string();
        }
        if e.take_name == old_name {
            e.take_name = new_name.to_string();
        }
        changed = true;
    }
    for t in &mut e.alt_takes {
        if t.media_id == old_id {
            t.media_id = new_id;
            if t.name == old_name {
                t.name = new_name.to_string();
            }
            changed = true;
        }
    }
    changed
}

/// Correções de takes ao abrir um projeto (§0.4): limite de takes, takes
/// alternativos sem mídia no projeto e posição do ativo fora da lista.
pub fn sanitize_takes(e: &mut Event, media: &HashSet<Id>) -> Vec<String> {
    let mut fixes = vec![];
    if e.alt_takes.len() >= MAX_TAKES {
        e.alt_takes.truncate(MAX_TAKES - 1);
        fixes.push(format!(
            "evento \"{}\" com takes demais: mantidos {MAX_TAKES}",
            e.name
        ));
    }
    let before = e.alt_takes.len();
    e.alt_takes.retain(|t| media.contains(&t.media_id));
    if e.alt_takes.len() != before {
        fixes.push(format!(
            "evento \"{}\": {} take(s) sem mídia no projeto removido(s)",
            e.name,
            before - e.alt_takes.len()
        ));
    }
    for t in &mut e.alt_takes {
        t.offset = t.offset.clamp_safe(Ticks::ZERO, Ticks::MAX);
    }
    if e.take_index as usize > e.alt_takes.len() {
        e.take_index = u32::try_from(e.alt_takes.len()).unwrap_or(0);
        fixes.push(format!(
            "evento \"{}\": posição do take ativo corrigida",
            e.name
        ));
    }
    fixes
}

/// Nome para mostrar de um take (o nome dele, ou o da mídia).
pub fn display_name(t: &Take, media_name: impl Fn(Id) -> Option<String>) -> String {
    if t.name.trim().is_empty() {
        media_name(t.media_id).unwrap_or_else(|| format!("mídia {}", t.media_id))
    } else {
        t.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaItem, MediaSource};

    fn media(id: Id) -> MediaItem {
        MediaItem {
            id,
            name: format!("m{id}"),
            source: MediaSource::File {
                path: format!("m{id}.avi"),
            },
            duration: Some(Ticks::from_secs_i64(60)),
            has_video: true,
            has_audio: false,
            width: 64,
            height: 36,
            fps: None,
            info: String::new(),
            proxy_path: None,
        }
    }

    fn take(id: Id) -> Take {
        Take {
            media_id: id,
            offset: Ticks::ZERO,
            name: String::new(),
        }
    }

    fn ev() -> Event {
        Event::new(&media(1), Ticks::ZERO, Ticks::from_secs_i64(5))
    }

    fn ids(e: &Event) -> Vec<Id> {
        takes(e).iter().map(|t| t.media_id).collect()
    }

    #[test]
    fn add_as_take_makes_the_new_one_active_and_keeps_the_old() {
        let mut e = ev();
        assert!(add_take(&mut e, take(2), true));
        assert_eq!(e.media_id, 2);
        assert_eq!(ids(&e), vec![1, 2]);
        assert_eq!(active_index(&e), 1);
        assert!(add_take(&mut e, take(3), false));
        assert_eq!(e.media_id, 2, "sem make_active o ativo não muda");
        assert_eq!(ids(&e), vec![1, 2, 3]);
    }

    #[test]
    fn next_and_prev_cycle_and_keep_order() {
        let mut e = ev();
        add_take(&mut e, take(2), false);
        add_take(&mut e, take(3), false);
        assert_eq!(e.media_id, 1);
        assert!(next_take(&mut e));
        assert_eq!(e.media_id, 2);
        assert!(next_take(&mut e));
        assert!(next_take(&mut e));
        assert_eq!(e.media_id, 1, "volta ao primeiro");
        assert!(prev_take(&mut e));
        assert_eq!(e.media_id, 3, "anterior do primeiro é o último");
        assert_eq!(ids(&e), vec![1, 2, 3], "a ordem não muda ao alternar");
        let mut single = ev();
        assert!(!next_take(&mut single));
        assert!(!prev_take(&mut single));
    }

    #[test]
    fn each_take_keeps_its_own_offset_and_name() {
        let mut e = ev();
        e.offset = Ticks::from_secs_i64(4);
        add_take(
            &mut e,
            Take {
                media_id: 2,
                offset: Ticks::from_secs_i64(9),
                name: "Câmera B".into(),
            },
            true,
        );
        assert_eq!(e.offset, Ticks::from_secs_i64(9));
        assert_eq!(e.take_name, "Câmera B");
        assert!(prev_take(&mut e));
        assert_eq!(e.offset, Ticks::from_secs_i64(4));
        assert_eq!(e.take_name, "");
    }

    #[test]
    fn delete_never_empties_and_moves_active_sensibly() {
        let mut e = ev();
        assert!(
            !delete_active(&mut e),
            "evento com um take não perde o take"
        );
        add_take(&mut e, take(2), false);
        add_take(&mut e, take(3), true);
        assert_eq!(e.media_id, 3);
        assert!(delete_active(&mut e));
        assert_eq!(e.media_id, 2, "apagando o último ativo, o anterior assume");
        assert!(delete_take(&mut e, 0));
        assert_eq!(ids(&e), vec![2]);
        assert!(!delete_take(&mut e, 0));
        assert!(!delete_take(&mut e, 9));
    }

    #[test]
    fn shift_moves_every_take_together() {
        let mut e = ev();
        add_take(&mut e, take(2), false);
        e.shift_offsets(Ticks::from_secs_i64(2));
        assert!(takes(&e)
            .iter()
            .all(|t| t.offset == Ticks::from_secs_i64(2)));
        assert!(next_take(&mut e));
        assert_eq!(
            e.offset,
            Ticks::from_secs_i64(2),
            "o outro take está no mesmo ponto"
        );
    }

    #[test]
    fn removing_a_media_keeps_the_event_when_another_take_exists() {
        let mut e = ev();
        add_take(&mut e, take(2), false);
        assert!(uses_media(&e, 2));
        assert!(remove_media(&mut e, 1), "o take 2 assume");
        assert_eq!(e.media_id, 2);
        assert_eq!(take_count(&e), 1);
        assert!(
            !remove_media(&mut e, 2),
            "sem outro take, o evento deve sair"
        );
    }

    #[test]
    fn sanitize_drops_missing_media_and_fixes_index() {
        let mut e = ev();
        add_take(&mut e, take(2), false);
        add_take(&mut e, take(99), false);
        e.take_index = 50;
        let media: HashSet<Id> = [1, 2].into_iter().collect();
        let fixes = sanitize_takes(&mut e, &media);
        assert_eq!(fixes.len(), 2, "{fixes:?}");
        assert_eq!(e.alt_takes.len(), 1);
        assert!(e.take_index as usize <= e.alt_takes.len());
        let mut many = ev();
        many.alt_takes = (0..200).map(|_| take(2)).collect();
        sanitize_takes(&mut many, &media);
        assert_eq!(take_count(&many), MAX_TAKES);
    }

    #[test]
    fn old_projects_without_takes_load_and_round_trip() {
        let e = ev();
        let json = serde_json::to_string(&e).expect("serializa");
        assert!(
            !json.contains("alt_takes"),
            "sem takes, nada novo no arquivo"
        );
        let back: Event = serde_json::from_str(&json).expect("lê");
        assert_eq!(take_count(&back), 1);
        let mut multi = ev();
        add_take(&mut multi, take(2), true);
        let back: Event =
            serde_json::from_str(&serde_json::to_string(&multi).expect("serializa")).expect("lê");
        assert_eq!(ids(&back), vec![1, 2]);
        assert_eq!(back.media_id, 2);
    }

    // ------------------------------------------------------------- sessão

    use crate::model::{Project, Track, TrackKind};
    use crate::session::EditorSession;

    /// Sessão com uma trilha de vídeo e um evento de 0 a 5 s com dois takes
    /// (mídias 1 e 2, ambos com offset 0), selecionado.
    fn session_with_takes() -> (EditorSession, Id) {
        let mut p = Project::new_default();
        p.media = vec![media(1), media(2), media(3)];
        let mut tr = Track::new(TrackKind::Video, "V");
        let mut e = ev();
        add_take(&mut e, take(2), false);
        let id = e.id;
        tr.events = vec![e];
        p.tracks = vec![tr];
        let mut s = EditorSession::new(p);
        s.sel_events.clear();
        s.sel_events.insert(id);
        (s, id)
    }

    fn offsets(s: &EditorSession, id: Id) -> Vec<Ticks> {
        s.project
            .event(id)
            .map(|e| takes(e).iter().map(|t| t.offset).collect())
            .unwrap_or_default()
    }

    #[test]
    fn split_shifts_every_take_of_the_right_part() {
        let (mut s, id) = session_with_takes();
        s.cursor = Ticks::from_secs_i64(2);
        assert_eq!(s.split_at_cursor(), 1);
        let right = s.project.tracks[0]
            .events
            .iter()
            .find(|e| e.id != id)
            .map(|e| e.id)
            .expect("parte direita");
        assert_eq!(
            offsets(&s, right),
            vec![Ticks::from_secs_i64(2); 2],
            "os dois takes começam no mesmo ponto da mídia"
        );
        assert_eq!(offsets(&s, id), vec![Ticks::ZERO; 2]);
    }

    #[test]
    fn trim_start_and_trim_to_selection_keep_takes_in_sync() {
        let (mut s, id) = session_with_takes();
        s.cursor = Ticks::from_secs_i64(1);
        s.trim_selected_start_to_cursor();
        assert_eq!(offsets(&s, id), vec![Ticks::from_secs_i64(1); 2]);
        s.time_sel = Some((Ticks::from_secs_i64(2), Ticks::from_secs_i64(4)));
        s.trim_to_selection();
        assert_eq!(offsets(&s, id), vec![Ticks::from_secs_i64(2); 2]);
    }

    #[test]
    fn media_used_only_by_an_inactive_take_is_not_unused() {
        let (mut s, _) = session_with_takes();
        let removed = s.remove_unused_media(None);
        assert_eq!(removed, vec![3], "só a mídia 3 não é usada");
        assert!(s.project.media.iter().any(|m| m.id == 2));
    }

    #[test]
    fn take_commands_are_one_undo_step_and_respect_locks() {
        let (mut s, id) = session_with_takes();
        let undo0 = s.undo.len();
        assert_eq!(s.next_take_selected(), 1);
        assert_eq!(s.project.event(id).map(|e| e.media_id), Some(2));
        assert_eq!(s.undo.len(), undo0 + 1);
        assert!(s.selection_has_takes());
        if let Some(e) = s.project.event_mut(id) {
            e.locked = true;
        }
        assert_eq!(s.prev_take_selected(), 0, "evento travado não muda");
        assert_eq!(s.undo.len(), undo0 + 1);
    }

    #[test]
    fn add_media_as_take_goes_to_matching_selected_events() {
        let (mut s, id) = session_with_takes();
        assert_eq!(s.add_media_as_take_to_selected(3), 1);
        let e = s.project.event(id).expect("evento");
        assert_eq!(e.media_id, 3, "o novo take vira o ativo");
        assert_eq!(take_count(e), 3);
        assert_eq!(s.add_media_as_take_to_selected(999), 0, "mídia inexistente");
    }

    #[test]
    fn sanitize_on_project_reports_missing_take_media() {
        let (mut s, id) = session_with_takes();
        if let Some(e) = s.project.event_mut(id) {
            e.alt_takes.push(take(77));
        }
        let fixes = s.project.sanitize();
        assert!(fixes.iter().any(|f| f.contains("sem mídia")), "{fixes:?}");
        assert_eq!(s.project.event(id).map(take_count), Some(2));
    }

    #[test]
    fn add_media_as_take_direto_no_evento_valida_bloqueio_e_compatibilidade() {
        let (mut s, id) = session_with_takes();
        assert!(s.add_media_as_take(id, 3));
        let e = s.project.event(id).expect("evento");
        assert_eq!(e.media_id, 3);
        assert_eq!(take_count(e), 3);

        // Mídia inexistente retorna false sem pânico
        assert!(!s.add_media_as_take(id, 99999));

        // Evento inexistente retorna false sem pânico
        assert!(!s.add_media_as_take(88888, 3));

        // Evento travado não aceita novo take
        if let Some(e) = s.project.event_mut(id) {
            e.locked = true;
        }
        assert!(!s.add_media_as_take(id, 2));
    }
}
