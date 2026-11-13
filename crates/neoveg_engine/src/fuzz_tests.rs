//! Testes de fuzzing por mutação (AGENTS.md §0.8): truncamento, inversão de
//! bits e tamanhos corrompidos, executados sob `catch_unwind`. Também os
//! testes de regressão de travamentos corrigidos.

use crate::media;
use crate::model::Project;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// Gerador pseudoaleatório determinístico (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Variações hostis de um arquivo válido.
fn mutations(valid: &[u8], count: usize, seed: u64) -> Vec<Vec<u8>> {
    let mut rng = Rng(seed | 1);
    let mut out = vec![];
    // truncamentos
    for k in 0..24 {
        let n = valid.len() * k / 24;
        out.push(valid[..n].to_vec());
    }
    for i in 0..count {
        let mut v = valid.to_vec();
        if v.is_empty() {
            break;
        }
        match i % 3 {
            // inversão de bits
            0 => {
                for _ in 0..1 + rng.below(8) {
                    let p = rng.below(v.len());
                    v[p] ^= 1 << rng.below(8);
                }
            }
            // campos de tamanho corrompidos (u32 alinhados)
            1 => {
                let p = rng.below(v.len() / 4) * 4;
                let val: u32 = match rng.below(5) {
                    0 => 0,
                    1 => u32::MAX,
                    2 => 0x7FFF_FFFF,
                    3 => 0xFFFF_FFF0,
                    _ => rng.next() as u32,
                };
                if p + 4 <= v.len() {
                    v[p..p + 4].copy_from_slice(&val.to_le_bytes());
                }
            }
            // bytes aleatórios numa região
            _ => {
                let p = rng.below(v.len());
                let n = (1 + rng.below(32)).min(v.len() - p);
                for b in &mut v[p..p + n] {
                    *b = rng.next() as u8;
                }
            }
        }
        out.push(v);
    }
    out
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("neoveg_fuzz_{}_{name}", std::process::id()))
}

fn assert_no_panic(what: &str, i: usize, f: impl FnOnce()) {
    let r = catch_unwind(AssertUnwindSafe(f));
    assert!(r.is_ok(), "{what}: pânico na mutação {i}");
}

fn valid_avi() -> Vec<u8> {
    let p = tmp("base.avi");
    let mut w =
        media::avi::AviWriter::create(&p, 32, 24, crate::time::FrameRate::FPS_25, Some(8000))
            .unwrap();
    for i in 0..6u8 {
        let img = image::RgbImage::from_pixel(32, 24, image::Rgb([i * 40, 100, 200]));
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 70)
            .encode_image(&img)
            .unwrap();
        w.write_video_jpeg(&jpg).unwrap();
        w.write_audio(&vec![500i16; 8000 / 25 * 2]).unwrap();
    }
    w.finish().unwrap();
    let b = std::fs::read(&p).unwrap();
    let _ = std::fs::remove_file(p);
    b
}

fn exercise_avi(bytes: &[u8], path: &PathBuf) {
    std::fs::write(path, bytes).unwrap();
    if let Ok(mut r) = media::avi::AviReader::open(path) {
        let n = r.frame_count();
        for i in [0, 1, n.saturating_sub(1), n + 5] {
            let _ = r.decode_frame(i);
        }
        let _ = r.read_audio();
    }
}

#[test]
fn fuzz_avi() {
    let base = valid_avi();
    let path = tmp("m.avi");
    for (i, m) in mutations(&base, 360, 0xA1).iter().enumerate() {
        assert_no_panic("AVI", i, || exercise_avi(m, &path));
    }
    let _ = std::fs::remove_file(path);
}

fn valid_y4m() -> Vec<u8> {
    let p = tmp("base.y4m");
    let mut w = media::y4m::Y4mWriter::create(&p, 16, 12, crate::time::FrameRate::FPS_25).unwrap();
    let px = vec![128u8; 16 * 12 * 4];
    for _ in 0..3 {
        w.write_frame(&px).unwrap();
    }
    w.finish().unwrap();
    let b = std::fs::read(&p).unwrap();
    let _ = std::fs::remove_file(p);
    b
}

fn exercise_y4m(bytes: &[u8], path: &PathBuf) {
    std::fs::write(path, bytes).unwrap();
    if let Ok(mut r) = media::y4m::Y4mReader::open(path) {
        for i in [0, 1, r.frames + 3] {
            let _ = r.decode_frame(i);
        }
    }
}

#[test]
fn fuzz_y4m() {
    let base = valid_y4m();
    let path = tmp("m.y4m");
    for (i, m) in mutations(&base, 300, 0xB2).iter().enumerate() {
        assert_no_panic("Y4M", i, || exercise_y4m(m, &path));
    }
    let _ = std::fs::remove_file(path);
}

fn valid_wav() -> Vec<u8> {
    let p = tmp("base.wav");
    let mut w = crate::render::WavWriter::create(&p, 8000, crate::render::WavBits::Pcm16).unwrap();
    let s: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
    w.write(&s).unwrap();
    w.finish().unwrap();
    let b = std::fs::read(&p).unwrap();
    let _ = std::fs::remove_file(p);
    b
}

#[test]
fn fuzz_wav() {
    let base = valid_wav();
    let path = tmp("m.wav");
    for (i, m) in mutations(&base, 150, 0xC3).iter().enumerate() {
        assert_no_panic("WAV", i, || {
            std::fs::write(&path, m).unwrap();
            let mut item = media_item(&path);
            let _ = media::load_entry(&mut item);
        });
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn fuzz_aiff_e_audio_corrompido() {
    // AIFF mínimo válido (FORM/AIFF/COMM/SSND) mutado em memória
    let mut aiff = Vec::new();
    aiff.extend_from_slice(b"FORM");
    aiff.extend_from_slice(&46u32.to_be_bytes());
    aiff.extend_from_slice(b"AIFF");
    aiff.extend_from_slice(b"COMM");
    aiff.extend_from_slice(&18u32.to_be_bytes());
    aiff.extend_from_slice(&1u16.to_be_bytes()); // canais
    aiff.extend_from_slice(&4u32.to_be_bytes()); // quadros
    aiff.extend_from_slice(&16u16.to_be_bytes()); // bits
    aiff.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]); // 44100 Hz
    aiff.extend_from_slice(b"SSND");
    aiff.extend_from_slice(&16u32.to_be_bytes());
    aiff.extend_from_slice(&0u32.to_be_bytes());
    aiff.extend_from_slice(&0u32.to_be_bytes());
    aiff.extend_from_slice(&[0, 0, 0x40, 0, 0x7F, 0xFF, 0x80, 0]);

    for (i, m) in mutations(&aiff, 150, 0xE5).iter().enumerate() {
        assert_no_panic("AIFF", i, || {
            let _ = media::wav::decode(m, 1024);
        });
    }

    // Bytes hostis nos decodificadores MP1/2/3, FLAC, OGG, AAC, M4A, CAF e MKA via load_entry (com guard)
    let mut rng = Rng(0xF6);
    for ext in [
        "mp3", "mp2", "mp1", "flac", "ogg", "oga", "aac", "m4a", "caf", "mka",
    ] {
        let p = tmp(&format!("corrompido.{ext}"));
        for i in 0..40usize {
            let len = (i * 17).min(512);
            let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            assert_no_panic(ext, i, || {
                std::fs::write(&p, &bytes).unwrap();
                let mut item = media_item(&p);
                let _ = media::load_entry(&mut item);
            });
        }
        let _ = std::fs::remove_file(p);
    }
}

#[test]
fn fuzz_symphonia_hostil() {
    let base = valid_wav();
    let path = tmp("sym_mut.wav");
    for (i, m) in mutations(&base, 80, 0x594D).iter().enumerate() {
        assert_no_panic("Symphonia WAV", i, || {
            let _ = std::fs::write(&path, m);
            let _ = media::audio::decode_symphonia(&path);
        });
    }
    let _ = std::fs::remove_file(&path);

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("fixtures")
        .join("sym_aac.m4a");
    if fixture.exists() {
        let bytes = std::fs::read(&fixture).unwrap();
        let m4a_path = tmp("sym_mut.m4a");
        for (i, m) in mutations(&bytes, 60, 0x4D34).iter().enumerate() {
            assert_no_panic("Symphonia M4A", i, || {
                let _ = std::fs::write(&m4a_path, m);
                let mut item = media_item(&m4a_path);
                let _ = media::load_entry(&mut item);
            });
        }
        let _ = std::fs::remove_file(m4a_path);
    }
}

#[test]
fn fuzz_gif() {
    let p = tmp("base.gif");
    {
        let f = std::fs::File::create(&p).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(f);
        for i in 0..3u8 {
            let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([i * 80, 0, 0, 255]));
            enc.encode_frame(image::Frame::new(img)).unwrap();
        }
    }
    let base = std::fs::read(&p).unwrap();
    for (i, m) in mutations(&base, 150, 0xD4).iter().enumerate() {
        assert_no_panic("GIF", i, || {
            std::fs::write(&p, m).unwrap();
            let mut item = media_item(&p);
            let _ = media::load_entry(&mut item);
        });
    }
    let _ = std::fs::remove_file(p);
}

fn media_item(path: &std::path::Path) -> crate::model::MediaItem {
    crate::model::MediaItem {
        id: 1,
        name: "x".into(),
        source: crate::model::MediaSource::File {
            path: path.to_string_lossy().to_string(),
        },
        duration: None,
        has_video: false,
        has_audio: false,
        width: 0,
        height: 0,
        fps: None,
        info: String::new(),
        proxy_path: None,
    }
}

const PROJECT_JSON: &str = r#"{
 "settings": {"width": 64, "height": 36, "fps": 30.0, "sample_rate": 48000},
 "media": [{"id": 1, "name": "t", "source": {"Generated": {"Text": {"text": "Oi", "size": 20.0}}}, "duration": null, "has_video": true, "has_audio": false}],
 "tracks": [{"id": 2, "name": "V", "kind": "Video", "events": [
   {"id": 3, "name": "t", "media_id": 1, "start": 0.0, "length": 2.0, "fade_in": {"len": 0.5, "curve": "Smooth"},
    "fx": [{"id": 4, "kind": "GaussianBlur", "enabled": true, "params": [{"value": 1.0, "keys": []}, {"value": 1.0, "keys": []}]}]}
 ], "envelopes": [{"kind": "Opacity", "points": [{"t": 0.0, "v": 1.0, "curve": "Linear"}, {"t": 2.0, "v": 0.5, "curve": "Fast"}]}]}]
}"#;

fn exercise_project(text: &str) {
    if let Ok((p, _)) = Project::from_json(text) {
        let cache = media::MediaCache::default();
        // resolução reduzida para o teste ser rápido
        for ms in [0, 400, 1000, 1900, 50_000] {
            let t = crate::time::Ticks::from_millis_i64(ms);
            let _ = crate::engine::video::render_frame(&p, &cache, t, 32, 18);
        }
        let mut mixer = crate::engine::audio::Mixer::new(8000);
        let mut buf = vec![0f32; 512];
        mixer.process(&p, &std::collections::HashMap::new(), 0, &mut buf);
    }
}

#[test]
fn fuzz_projeto_json() {
    let base = PROJECT_JSON.as_bytes();
    for (i, m) in mutations(base, 300, 0xE5).iter().enumerate() {
        let text = String::from_utf8_lossy(m).to_string();
        assert_no_panic("projeto", i, || exercise_project(&text));
    }
}

// ---------------------------------------------------------------------------
// Regressões de travamentos corrigidos
// ---------------------------------------------------------------------------

/// Antes: o id do chunk era convertido com `from_utf8_lossy` e cortado com
/// `split_at(2)`, que entrava em pânico com bytes não ASCII.
#[test]
fn regressao_avi_id_de_chunk_nao_utf8() {
    let mut b = valid_avi();
    let pos = b.windows(4).position(|w| w == b"00dc").unwrap();
    b[pos] = 0xFF;
    let path = tmp("reg_id.avi");
    assert_no_panic("AVI id não UTF-8", 0, || exercise_avi(&b, &path));
    let _ = std::fs::remove_file(path);
}

/// Antes: uma lista `hdrl` declarando ~4 GB era alocada inteira, derrubando o
/// processo por falta de memória.
#[test]
fn regressao_avi_hdrl_gigante_nao_aloca() {
    let mut b = valid_avi();
    let pos = b.windows(4).position(|w| w == b"hdrl").unwrap() - 4;
    b[pos..pos + 4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    let path = tmp("reg_hdrl.avi");
    std::fs::write(&path, &b).unwrap();
    let r = media::avi::AviReader::open(&path);
    // o importante é não abortar por falta de memória; erro ou leitura parcial são aceitáveis
    drop(r);
    let _ = std::fs::remove_file(path);
}

/// Antes: `split_at(1)` num token começando por caractere multibyte entrava em pânico.
#[test]
fn regressao_y4m_token_multibyte() {
    let mut b = b"YUV4MPEG2 W16 H12 \xC3\x84X F25:1 C420jpeg\n".to_vec();
    b.extend_from_slice(b"FRAME\n");
    b.extend(std::iter::repeat_n(128u8, 16 * 12 * 3 / 2));
    let path = tmp("reg_tok.y4m");
    assert_no_panic("Y4M token multibyte", 0, || exercise_y4m(&b, &path));
    let _ = std::fs::remove_file(path);
}

/// Antes: dimensões enormes no cabeçalho geravam uma alocação de centenas de
/// MB mesmo sem dados no arquivo.
#[test]
fn regressao_y4m_dimensoes_hostis() {
    let mut b = b"YUV4MPEG2 W4000000000 H4000000000 F25:1\nFRAME\n".to_vec();
    b.extend_from_slice(&[0u8; 64]);
    let path = tmp("reg_dim.y4m");
    std::fs::write(&path, &b).unwrap();
    assert!(media::y4m::Y4mReader::open(&path).is_err());
    let b = b"YUV4MPEG2 W16000 H16000 F25:1\nFRAME\nxx".to_vec();
    std::fs::write(&path, &b).unwrap();
    assert!(media::y4m::Y4mReader::open(&path).is_err());
    let _ = std::fs::remove_file(path);
}

/// Fades maiores que o evento, NaN e resoluções absurdas vindos do arquivo
/// são corrigidos antes de chegar ao motor (e a `clamp` com limites cruzados).
#[test]
fn regressao_projeto_valores_hostis() {
    let text = PROJECT_JSON
        .replace(r#""width": 64"#, r#""width": 4000000000"#)
        .replace(r#""fps": 30.0"#, r#""fps": 0.0"#)
        .replace(r#""fade_in": {"len": 0.5"#, r#""fade_in": {"len": 50.0"#)
        .replace(r#""length": 2.0"#, r#""length": -3.0"#);
    let (p, fixes) = Project::from_json(&text).unwrap();
    assert!(!fixes.is_empty());
    assert!(p.settings.width <= crate::model::MAX_DIM);
    assert!(p.settings.fps.as_f64() >= 1.0);
    let e = &p.tracks[0].events[0];
    assert!(e.length > crate::time::Ticks::ZERO);
    assert!(e.fade_in.len + e.fade_out.len <= e.length);
    exercise_project(&text);
}

/// Achado do fuzzer: um GIF com tela lógica corrompida (ex.: 65535x65535)
/// fazia o compositor de quadros alocar ~17 GB e abortar o processo.
#[test]
fn regressao_gif_tela_gigante_nao_aloca() {
    let p = tmp("reg_gif.gif");
    {
        let f = std::fs::File::create(&p).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(f);
        for _ in 0..2 {
            let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([9, 9, 9, 255]));
            enc.encode_frame(image::Frame::new(img)).unwrap();
        }
    }
    let mut b = std::fs::read(&p).unwrap();
    b[6..10].copy_from_slice(&[0xF0, 0xFF, 0xFF, 0xFF]);
    std::fs::write(&p, &b).unwrap();
    let mut item = media_item(&p);
    assert!(media::load_entry(&mut item).is_err());
    let _ = std::fs::remove_file(p);
}

#[test]
fn fuzz_tempo_exato_e_parse_hostis() {
    use crate::time::{parse_time, FrameRate, TimeFormat};

    let hostis = [
        "",
        "   ",
        "-1",
        "NaN",
        "inf",
        "-inf",
        "99999999999999999999999",
        "9999999999999999999f",
        "00:00:00;00;00:00",
        "100000:00:00;99",
        "00:00:00;00",
        "00:01:00;00",
        "00:01:00;01",
        "00:10:00;00",
        "abc:def:ghi.jkl",
        ":::...",
        "-9223372036854775808s",
        "9223372036854775807f",
    ];
    for fmt in TimeFormat::ALL {
        for fps in FrameRate::STANDARD {
            for s in hostis {
                assert_no_panic("parse_time", 0, || {
                    let _ = parse_time(s, fps, 48000, fmt);
                });
            }
        }
    }

    // Valores extremos no JSON do projeto não devem causar pânico nem divisão por zero
    let json_extremo = PROJECT_JSON
        .replace(r#""fps": 30.0"#, r#""fps": {"num": 0, "den": 0}"#)
        .replace(
            r#""length": 2.0"#,
            r#""length": {"ticks": 9223372036854775807}"#,
        )
        .replace(
            r#""start": 0.0"#,
            r#""start": {"ticks": -9223372036854775808}"#,
        );
    assert_no_panic("project_json_extremo", 0, || {
        exercise_project(&json_extremo);
    });
}

#[test]
fn fuzz_mp4_e_media_foundation_hostil() {
    // Bytes MP4/ISO-BMFF mínimos sintéticos + mutações hostis (§0.8).
    let mut base = Vec::new();
    base.extend_from_slice(&20u32.to_be_bytes());
    base.extend_from_slice(b"ftypisom");
    base.extend_from_slice(&0u32.to_be_bytes());
    base.extend_from_slice(b"isom");
    base.extend_from_slice(&16u32.to_be_bytes());
    base.extend_from_slice(b"moov");
    base.extend_from_slice(&[0u8; 8]);
    base.extend_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
    base.extend_from_slice(b"mdat");
    base.extend_from_slice(&[0x41u8; 64]);

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("fixtures")
        .join("mf_smoke.mp4");

    let mp4_bytes = if fixture.exists() {
        let mut item = media_item(&fixture);
        let imp = media::load_entry(&mut item).expect("fixture mf_smoke.mp4 deveria carregar");
        assert!(imp.item.has_video);
        assert!(imp.item.width > 0 && imp.item.height > 0);
        let cache = media::MediaCache::default();
        cache.insert(imp.item.id, imp.entry);
        let img = cache.video_frame(&imp.item, crate::time::Ticks::from_millis_i64(500), 160, 90);
        assert!(
            img.is_some(),
            "quadro decodificado via Media Foundation esperado"
        );
        std::fs::read(&fixture).unwrap()
    } else {
        base
    };

    let mut_path = tmp("hostil.mp4");
    for (i, m) in mutations(&mp4_bytes, 40, 0x4D46).iter().enumerate() {
        assert_no_panic("MP4/MF", i, || {
            let _ = std::fs::write(&mut_path, m);
            let mut item = media_item(&mut_path);
            let _ = media::load_entry(&mut item);
        });
    }
    let _ = std::fs::remove_file(mut_path);
}
