//! Janelas que um script pede durante a execução (`MessageBox`, `Form`,
//! diálogos de arquivo), fora do WinForms real.
//!
//! No Linux e no macOS (e no Windows sem o runtime WindowsDesktop), a ponte
//! C# traz uma camada `System.Windows.Forms` própria, com os nomes da
//! documentação pública da Microsoft (learn.microsoft.com). Quando o script
//! mostra uma janela, essa camada descreve a janela em JSON e a entrega ao
//! Neoveg por [`ScriptUi`]; quem desenha é a interface do Neoveg. Sem
//! interface (CLI, testes sem respondedor), a resposta é [`ScriptUiReply::NoUi`]
//! e a camada usa o padrão: primeiro botão da caixa de mensagem, Cancelar
//! nos formulários e diálogos de arquivo.
//!
//! Tudo o que vem do script é entrada hostil (§0.4): [`ScriptUiRequest::sanitize`]
//! limita quantidade de controles, itens, textos e coordenadas antes de a
//! interface ver o pedido.

use serde::{Deserialize, Serialize};

/// Maior pedido aceito do script, em bytes de JSON.
pub(crate) const MAX_UI_REQUEST_BYTES: usize = 1024 * 1024;
/// Limites de um formulário.
const MAX_CONTROLS: usize = 256;
const MAX_ITEMS: usize = 1000;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_FORM_SIDE: f32 = 4000.0;

/// Quem mostra as janelas do script. Chamado na thread do script; bloqueia
/// até o usuário responder.
pub trait ScriptUi: Send + Sync {
    fn request(&self, request: ScriptUiRequest) -> ScriptUiReply;
}

/// Botões de uma caixa de mensagem (`MessageBoxButtons`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageButtons {
    #[serde(rename = "OK")]
    Ok,
    #[serde(rename = "OKCancel")]
    OkCancel,
    AbortRetryIgnore,
    YesNoCancel,
    YesNo,
    RetryCancel,
    CancelTryContinue,
}

impl MessageButtons {
    /// Nomes de `DialogResult` dos botões, na ordem em que aparecem.
    pub fn results(self) -> &'static [&'static str] {
        match self {
            Self::Ok => &["OK"],
            Self::OkCancel => &["OK", "Cancel"],
            Self::AbortRetryIgnore => &["Abort", "Retry", "Ignore"],
            Self::YesNoCancel => &["Yes", "No", "Cancel"],
            Self::YesNo => &["Yes", "No"],
            Self::RetryCancel => &["Retry", "Cancel"],
            Self::CancelTryContinue => &["Cancel", "TryAgain", "Continue"],
        }
    }
}

/// Tipo de controle de um formulário.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Label,
    TextBox,
    Button,
    CheckBox,
    RadioButton,
    ComboBox,
    ListBox,
    Numeric,
    GroupBox,
    Panel,
}

/// Um controle, já em coordenadas do formulário (os contêineres somam o
/// deslocamento dos filhos).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ControlSpec {
    pub id: u32,
    pub kind: Option<ControlKind>,
    /// Contêiner (`GroupBox`/`Panel`) do controle; agrupa os `RadioButton`.
    pub parent: Option<u32>,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    pub items: Vec<String>,
    pub selected: i32,
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub decimals: u32,
    pub increment: f64,
    pub multiline: bool,
    pub password: bool,
    pub read_only: bool,
    /// Lista só para escolha (`ComboBoxStyle.DropDownList`).
    pub drop_down_list: bool,
    /// O script tem handlers de mudança/clique: avisar assim que mudar.
    pub notify: bool,
    /// Botão que fecha o formulário com este `DialogResult`.
    pub dialog_result: Option<String>,
}

/// Formulário mostrado com `Form.ShowDialog()`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct FormSpec {
    pub title: String,
    /// Área útil (`ClientSize`).
    pub width: f32,
    pub height: f32,
    pub controls: Vec<ControlSpec>,
    /// `AcceptButton` (Enter) e `CancelButton` (Esc).
    pub accept: Option<u32>,
    pub cancel: Option<u32>,
}

/// Diálogo de arquivo ou pasta.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileDialogMode {
    #[default]
    Open,
    Save,
    Folder,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct FileDialogSpec {
    pub mode: FileDialogMode,
    pub title: String,
    /// Filtro no formato do WinForms: `"Texto|*.txt|Todos|*.*"`.
    pub filter: String,
    pub initial_dir: String,
    pub file_name: String,
    pub multiselect: bool,
}

impl FileDialogSpec {
    /// Pares (nome, extensões sem `*.`) do filtro do WinForms; `*.*` vira
    /// "qualquer arquivo" e não entra.
    pub fn filters(&self) -> Vec<(String, Vec<String>)> {
        let parts: Vec<&str> = self.filter.split('|').collect();
        parts
            .chunks(2)
            .filter_map(|c| {
                let (name, pat) = (c.first()?, c.get(1)?);
                let exts: Vec<String> = pat
                    .split(';')
                    .filter_map(|p| p.trim().strip_prefix("*."))
                    .filter(|e| !e.is_empty() && *e != "*")
                    .map(str::to_string)
                    .collect();
                (!exts.is_empty()).then(|| (name.trim().to_string(), exts))
            })
            .take(64)
            .collect()
    }
}

/// O que o script pediu para mostrar.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScriptUiRequest {
    MessageBox {
        #[serde(default)]
        text: String,
        #[serde(default)]
        caption: String,
        buttons: MessageButtons,
        #[serde(default)]
        icon: String,
    },
    Form(FormSpec),
    FileDialog(FileDialogSpec),
}

/// Valor de um controle depois da interação.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ControlValue {
    pub id: u32,
    pub text: String,
    pub checked: bool,
    pub selected: i32,
    pub value: f64,
}

/// Resposta da interface.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScriptUiReply {
    /// Não há interface: a camada usa o padrão.
    NoUi,
    MessageBox {
        result: String,
    },
    /// O formulário fechou (`closed`, com `result`) ou um controle avisou
    /// uma mudança/clique (`clicked`); `values` traz o estado de todos.
    Form {
        closed: bool,
        result: Option<String>,
        clicked: Option<u32>,
        values: Vec<ControlValue>,
    },
    File {
        ok: bool,
        paths: Vec<String>,
    },
}

fn clip(s: &mut String) {
    if s.len() > MAX_TEXT_BYTES {
        let mut end = MAX_TEXT_BYTES;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// Valor na faixa; NaN/infinito vira o mínimo da faixa, sem ficar negativo
/// (largura NaN vira a largura mínima; coordenada NaN vira 0).
fn finite(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        lo.max(0.0).min(hi)
    }
}

impl ScriptUiRequest {
    /// Limita o pedido (§0.4): controles, itens, textos, coordenadas e
    /// faixas numéricas válidas.
    pub fn sanitize(&mut self) {
        match self {
            Self::MessageBox { text, caption, .. } => {
                clip(text);
                clip(caption);
            }
            Self::Form(f) => {
                clip(&mut f.title);
                f.width = finite(f.width, 80.0, MAX_FORM_SIDE);
                f.height = finite(f.height, 40.0, MAX_FORM_SIDE);
                f.controls.truncate(MAX_CONTROLS);
                for c in &mut f.controls {
                    clip(&mut c.text);
                    c.items.truncate(MAX_ITEMS);
                    c.items.iter_mut().for_each(clip);
                    c.x = finite(c.x, -MAX_FORM_SIDE, MAX_FORM_SIDE);
                    c.y = finite(c.y, -MAX_FORM_SIDE, MAX_FORM_SIDE);
                    c.w = finite(c.w, 0.0, MAX_FORM_SIDE);
                    c.h = finite(c.h, 0.0, MAX_FORM_SIDE);
                    if !c.minimum.is_finite() {
                        c.minimum = 0.0;
                    }
                    if !c.maximum.is_finite() || c.maximum < c.minimum {
                        c.maximum = c.minimum;
                    }
                    c.value = if c.value.is_finite() {
                        c.value.clamp(c.minimum, c.maximum)
                    } else {
                        c.minimum
                    };
                    if !c.increment.is_finite() || c.increment <= 0.0 {
                        c.increment = 1.0;
                    }
                    c.decimals = c.decimals.min(10);
                    let n = i32::try_from(c.items.len()).unwrap_or(i32::MAX);
                    if c.selected < -1 || c.selected >= n {
                        c.selected = -1;
                    }
                }
            }
            Self::FileDialog(d) => {
                clip(&mut d.title);
                clip(&mut d.filter);
                clip(&mut d.initial_dir);
                clip(&mut d.file_name);
            }
        }
    }
}

/// Lê, limita e entrega um pedido do script; devolve a resposta em JSON.
pub(crate) fn handle_request(ui: Option<&dyn ScriptUi>, raw: &[u8]) -> Vec<u8> {
    let reply = if raw.len() > MAX_UI_REQUEST_BYTES {
        Err("pedido de janela grande demais".to_string())
    } else {
        serde_json::from_slice::<ScriptUiRequest>(raw)
            .map_err(|e| format!("pedido de janela inválido: {e}"))
            .map(|mut req| {
                req.sanitize();
                match ui {
                    Some(ui) => ui.request(req),
                    None => ScriptUiReply::NoUi,
                }
            })
    };
    let json = match reply {
        Ok(r) => serde_json::to_vec(&r),
        Err(e) => serde_json::to_vec(&serde_json::json!({ "kind": "error", "error": e })),
    };
    json.unwrap_or_else(|_| br#"{"kind":"no_ui"}"#.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pedido_hostil_e_limitado() {
        let mut req = ScriptUiRequest::Form(FormSpec {
            title: "x".repeat(MAX_TEXT_BYTES + 10),
            width: f32::NAN,
            height: 1.0e9,
            controls: (0..1000)
                .map(|i| ControlSpec {
                    id: i,
                    x: f32::INFINITY,
                    w: -5.0,
                    minimum: 10.0,
                    maximum: f64::NAN,
                    value: 99.0,
                    increment: -1.0,
                    selected: 7,
                    items: vec!["a".into(); 2000],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        req.sanitize();
        let ScriptUiRequest::Form(f) = req else {
            panic!("formulário");
        };
        assert_eq!(f.title.len(), MAX_TEXT_BYTES);
        assert_eq!((f.width, f.height), (80.0, MAX_FORM_SIDE));
        assert_eq!(f.controls.len(), MAX_CONTROLS);
        let c = &f.controls[0];
        assert_eq!((c.x, c.w), (0.0, 0.0));
        assert_eq!(
            (c.minimum, c.maximum, c.value, c.increment),
            (10.0, 10.0, 10.0, 1.0)
        );
        assert_eq!(c.items.len(), MAX_ITEMS);
        assert_eq!(c.selected, 7);
        let mut fora = ScriptUiRequest::Form(FormSpec {
            controls: vec![ControlSpec {
                selected: 3,
                ..Default::default()
            }],
            ..Default::default()
        });
        fora.sanitize();
        let ScriptUiRequest::Form(f) = fora else {
            panic!("formulário");
        };
        assert_eq!(f.controls[0].selected, -1, "seleção fora da lista");
    }

    #[test]
    fn json_invalido_ou_grande_vira_erro_sem_panico() {
        for raw in [
            b"{".to_vec(),
            br#"{"kind":"message_box","buttons":"Bogus"}"#.to_vec(),
            vec![b' '; MAX_UI_REQUEST_BYTES + 1],
        ] {
            let out = handle_request(None, &raw);
            let v: serde_json::Value = serde_json::from_slice(&out).expect("json");
            assert_eq!(v["kind"], "error");
        }
        let ok = handle_request(
            None,
            br#"{"kind":"message_box","text":"oi","buttons":"YesNo"}"#,
        );
        let r: ScriptUiReply = serde_json::from_slice(&ok).expect("resposta");
        assert_eq!(r, ScriptUiReply::NoUi);
    }

    #[test]
    fn filtro_do_winforms_vira_extensoes() {
        let d = FileDialogSpec {
            filter: "Vídeo|*.mp4;*.mov|Todos|*.*".into(),
            ..Default::default()
        };
        assert_eq!(
            d.filters(),
            vec![(
                "Vídeo".to_string(),
                vec!["mp4".to_string(), "mov".to_string()]
            )]
        );
        assert!(FileDialogSpec::default().filters().is_empty());
    }
}
