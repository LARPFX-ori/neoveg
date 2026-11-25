//! Ferramenta mão das vistas (Prévia de Vídeo com zoom, Pan/Crop e Movimento
//! da Trilha).
//!
//! Com o ponteiro sobre uma dessas vistas, segurar Espaço vira a mão
//! temporária: arrastar com o botão esquerdo desloca a vista. Soltar o Espaço
//! sem ter arrastado faz o que o Espaço sempre faz (Reproduzir / Pausar), então
//! o atalho do VEGAS continua valendo; só o arrasto com Espaço é da mão.
//!
//! O estado fica na memória temporária do `egui::Context`, compartilhada pelas
//! janelas destacadas.

use eframe::egui;
use std::time::{Duration, Instant};

/// Por quanto tempo depois do último quadro com o ponteiro sobre a vista o
/// Espaço ainda vale como mão.
const HOVER_GRACE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Default)]
struct SpaceHand {
    hover_at: Option<Instant>,
    /// Espaço apertado sobre uma vista: reproduzir fica para a soltura.
    armed: bool,
    /// Houve arrasto com a mão desde que o Espaço foi apertado.
    dragged: bool,
}

fn key() -> egui::Id {
    egui::Id::new("neoveg_space_hand")
}

fn load(ctx: &egui::Context) -> SpaceHand {
    ctx.data(|d| d.get_temp::<SpaceHand>(key()))
        .unwrap_or_default()
}

fn store(ctx: &egui::Context, s: SpaceHand) {
    ctx.data_mut(|d| d.insert_temp(key(), s));
}

/// A vista que aceita a mão está sob o ponteiro neste quadro.
pub(crate) fn note_hover(ctx: &egui::Context) {
    let mut s = load(ctx);
    s.hover_at = Some(Instant::now());
    store(ctx, s);
}

/// A mão deslocou a vista: soltar o Espaço não deve reproduzir.
pub(crate) fn note_drag(ctx: &egui::Context) {
    let mut s = load(ctx);
    s.dragged = true;
    store(ctx, s);
}

/// Espaço segurado, sem modificadores e fora de um campo de texto.
pub(crate) fn space_held(ui: &egui::Ui) -> bool {
    // lido fora de `ui.input`: dois read-locks aninhados no contexto podem
    // travar com a thread de prévia (ver `kf_window`)
    let typing = ui.ctx().wants_keyboard_input();
    !typing && ui.input(|i| i.key_down(egui::Key::Space) && i.modifiers.is_none())
}

/// A mão vale nesta vista: ferramenta mão escolhida ou Espaço segurado.
/// Registra o ponteiro sobre a vista para o atalho do Espaço.
pub(crate) fn hand_active(ui: &egui::Ui, resp: &egui::Response, tool: bool) -> bool {
    if resp.hovered() {
        note_hover(ui.ctx());
    }
    tool || space_held(ui)
}

/// O que o Espaço deve fazer neste quadro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpaceDecision {
    /// Nada com a mão: o Espaço segue o atalho normal.
    Normal,
    /// O Espaço é da mão: não executar o atalho agora.
    Consumed,
    /// Soltou sem arrastar: executar agora o atalho do Espaço.
    Release,
}

/// Decide o Espaço da janela principal. `no_mods`: sem Ctrl, Shift ou Alt.
pub(crate) fn space_shortcut(ctx: &egui::Context, no_mods: bool) -> SpaceDecision {
    let (pressed, first_press, released) = ctx.input(|i| {
        let mut pressed = false;
        let mut first = false;
        let mut released = false;
        for e in &i.events {
            if let egui::Event::Key {
                key: egui::Key::Space,
                pressed: p,
                repeat,
                ..
            } = e
            {
                if *p {
                    pressed = true;
                    first |= !*repeat;
                } else {
                    released = true;
                }
            }
        }
        (pressed, first, released)
    });
    let mut s = load(ctx);
    let over_view = s.hover_at.is_some_and(|t| t.elapsed() <= HOVER_GRACE);
    let mut out = SpaceDecision::Normal;
    if pressed && s.armed {
        // repetição da tecla segurada
        out = SpaceDecision::Consumed;
    } else if pressed && first_press && no_mods && over_view {
        s.armed = true;
        s.dragged = false;
        out = SpaceDecision::Consumed;
    }
    if released && s.armed {
        out = if s.dragged {
            SpaceDecision::Consumed
        } else {
            SpaceDecision::Release
        };
        s.armed = false;
        s.dragged = false;
    }
    store(ctx, s);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn decide(ctx: &egui::Context, events: Vec<egui::Event>) -> SpaceDecision {
        let mut out = SpaceDecision::Normal;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            out = space_shortcut(ctx, true);
        });
        out
    }

    #[test]
    fn espaco_fora_da_vista_segue_o_atalho() {
        let ctx = egui::Context::default();
        assert_eq!(
            decide(&ctx, vec![key_event(true, false)]),
            SpaceDecision::Normal
        );
        assert_eq!(
            decide(&ctx, vec![key_event(false, false)]),
            SpaceDecision::Normal
        );
    }

    #[test]
    fn espaco_sobre_a_vista_sem_arrasto_reproduz_ao_soltar() {
        let ctx = egui::Context::default();
        note_hover(&ctx);
        assert_eq!(
            decide(&ctx, vec![key_event(true, false)]),
            SpaceDecision::Consumed
        );
        assert_eq!(
            decide(&ctx, vec![key_event(true, true)]),
            SpaceDecision::Consumed
        );
        assert_eq!(
            decide(&ctx, vec![key_event(false, false)]),
            SpaceDecision::Release
        );
        // toque rápido: apertar e soltar no mesmo quadro
        note_hover(&ctx);
        assert_eq!(
            decide(&ctx, vec![key_event(true, false), key_event(false, false)]),
            SpaceDecision::Release
        );
    }

    #[test]
    fn espaco_com_arrasto_da_mao_nao_reproduz() {
        let ctx = egui::Context::default();
        note_hover(&ctx);
        assert_eq!(
            decide(&ctx, vec![key_event(true, false)]),
            SpaceDecision::Consumed
        );
        note_drag(&ctx);
        assert_eq!(
            decide(&ctx, vec![key_event(false, false)]),
            SpaceDecision::Consumed
        );
        // o próximo Espaço começa limpo
        note_hover(&ctx);
        assert_eq!(
            decide(&ctx, vec![key_event(true, false)]),
            SpaceDecision::Consumed
        );
        assert_eq!(
            decide(&ctx, vec![key_event(false, false)]),
            SpaceDecision::Release
        );
    }
}
