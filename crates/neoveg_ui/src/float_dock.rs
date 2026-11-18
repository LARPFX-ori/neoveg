//! Acoplar uma janela destacada arrastando-a para a área de janelas
//! (manual do VEGAS Pro 21, p. 42-43: "Docking a window" e "To prevent a
//! window from docking").
//!
//! As janelas destacadas são janelas nativas do sistema: o egui não avisa
//! quando o usuário solta a barra de título. Por isso o fim do arraste é
//! deduzido da posição real da janela a cada quadro: ela se moveu e depois
//! ficou parada por [`SETTLE`]. Se nesse momento o ponto de pega da barra de
//! título estiver sobre a área de janelas, a janela acopla conforme a
//! preferência *Permitir que janelas flutuantes acoplem*:
//! ligada, acopla a menos que `Ctrl` esteja apertado; desligada, só com
//! `Ctrl`.

use eframe::egui::{pos2, Pos2, Rect};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Tempo parada que conta como "soltou a janela".
pub const SETTLE: Duration = Duration::from_millis(250);
/// Carência depois que a janela aparece: o sistema pode reposicioná-la ao
/// abrir, e isso não é um arraste do usuário (sem ela, uma janela aberta em
/// cima da área de janelas acoplaria de volta sozinha).
pub const OPEN_GRACE: Duration = Duration::from_millis(600);
/// Altura (pontos) abaixo do canto de cima onde fica a barra de título.
const TITLE_GRAB_Y: f32 = 12.0;

#[derive(Clone, Copy, Debug)]
struct MoveTrack {
    pos: Pos2,
    moved_at: Option<Instant>,
    seen_at: Instant,
}

/// Movimento das janelas destacadas, por chave.
#[derive(Debug, Default)]
pub struct FloatDockTracker {
    tracks: HashMap<String, MoveTrack>,
}

/// Estado de uma janela neste quadro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveState {
    /// Parada (ou ainda sem posição conhecida).
    Idle,
    /// Em movimento: mostre o contorno da área se for acoplar.
    Moving,
    /// Acabou de parar depois de se mover: decida se acopla.
    Settled,
}

/// Ponto da barra de título usado para decidir onde a janela está.
pub fn grab_point(outer: Rect) -> Pos2 {
    pos2(outer.center().x, outer.top() + TITLE_GRAB_Y)
}

/// A janela deve acoplar? `allow` é a preferência, `ctrl` o estado da tecla.
pub fn wants_dock(allow: bool, ctrl: bool) -> bool {
    allow != ctrl
}

impl FloatDockTracker {
    /// Atualiza a janela `key` com a posição (canto de fora) neste quadro.
    pub fn update(&mut self, key: &str, pos: Pos2, now: Instant) -> MoveState {
        if !pos.x.is_finite() || !pos.y.is_finite() {
            return MoveState::Idle;
        }
        let Some(t) = self.tracks.get_mut(key) else {
            self.tracks.insert(
                key.to_string(),
                MoveTrack {
                    pos,
                    moved_at: None,
                    seen_at: now,
                },
            );
            return MoveState::Idle;
        };
        if (t.pos - pos).length() > 0.5 {
            t.pos = pos;
            if now.saturating_duration_since(t.seen_at) < OPEN_GRACE {
                return MoveState::Idle;
            }
            t.moved_at = Some(now);
            return MoveState::Moving;
        }
        match t.moved_at {
            Some(at) if now.saturating_duration_since(at) >= SETTLE => {
                t.moved_at = None;
                MoveState::Settled
            }
            Some(_) => MoveState::Moving,
            None => MoveState::Idle,
        }
    }

    /// Esquece a janela (fechou ou acoplou).
    pub fn forget(&mut self, key: &str) {
        self.tracks.remove(key);
    }

    /// Esquece as janelas que não estão em `alive`.
    pub fn retain(&mut self, alive: &std::collections::HashSet<String>) {
        self.tracks.retain(|k, _| alive.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn mover_e_parar_vira_soltar_uma_vez() {
        let mut t = FloatDockTracker::default();
        let t0 = Instant::now();
        assert_eq!(t.update("mixer", pos2(10.0, 10.0), t0), MoveState::Idle);
        assert_eq!(
            t.update("mixer", pos2(10.0, 10.0), t0 + SETTLE * 2),
            MoveState::Idle,
            "nunca se moveu"
        );
        // o sistema reposiciona a janela logo ao abrir: não é arraste
        assert_eq!(
            t.update("mixer", pos2(12.0, 30.0), t0 + OPEN_GRACE / 2),
            MoveState::Idle
        );
        let t1 = t0 + OPEN_GRACE + SETTLE * 3;
        assert_eq!(t.update("mixer", pos2(40.0, 10.0), t1), MoveState::Moving);
        assert_eq!(
            t.update("mixer", pos2(40.0, 10.0), t1 + SETTLE / 2),
            MoveState::Moving
        );
        assert_eq!(
            t.update("mixer", pos2(40.0, 10.0), t1 + SETTLE),
            MoveState::Settled
        );
        assert_eq!(
            t.update("mixer", pos2(40.0, 10.0), t1 + SETTLE * 4),
            MoveState::Idle,
            "só uma vez"
        );
        // tremida abaixo de meio ponto não é movimento
        assert_eq!(
            t.update("mixer", pos2(40.2, 10.1), t1 + SETTLE * 5),
            MoveState::Idle
        );
        // posição inválida não muda nada
        assert_eq!(t.update("mixer", pos2(f32::NAN, 0.0), t1), MoveState::Idle);
        t.forget("mixer");
        assert_eq!(
            t.update("mixer", pos2(99.0, 9.0), t1),
            MoveState::Idle,
            "recomeça"
        );
    }

    #[test]
    fn regra_do_ctrl_e_ponto_da_barra_de_titulo() {
        assert!(wants_dock(true, false));
        assert!(!wants_dock(true, true), "Ctrl impede com a opção ligada");
        assert!(!wants_dock(false, false));
        assert!(wants_dock(false, true), "só com Ctrl com a opção desligada");
        let r = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 300.0));
        assert_eq!(grab_point(r), pos2(300.0, 62.0));
    }

    #[test]
    fn retain_esquece_janelas_fechadas() {
        let mut t = FloatDockTracker::default();
        let t0 = Instant::now();
        t.update("a", pos2(0.0, 0.0), t0);
        t.update("b", pos2(0.0, 0.0), t0);
        let vivas: std::collections::HashSet<String> = ["a".to_string()].into_iter().collect();
        t.retain(&vivas);
        let depois = t0 + OPEN_GRACE * 2;
        assert_eq!(
            t.update("b", pos2(5.0, 0.0), depois),
            MoveState::Idle,
            "b foi esquecida"
        );
        assert_eq!(t.update("a", pos2(5.0, 0.0), depois), MoveState::Moving);
    }
}
