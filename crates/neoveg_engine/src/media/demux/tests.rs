use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::Command;

fn push_be_u16(dst: &mut Vec<u8>, v: u16) {
    dst.extend_from_slice(&v.to_be_bytes());
}

fn push_be_u32(dst: &mut Vec<u8>, v: u32) {
    dst.extend_from_slice(&v.to_be_bytes());
}

fn push_be_u64(dst: &mut Vec<u8>, v: u64) {
    dst.extend_from_slice(&v.to_be_bytes());
}

fn mp4_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let total = (8 + payload.len()) as u32;
    let mut out = Vec::with_capacity(total as usize);
    push_be_u32(&mut out, total);
    out.extend_from_slice(fourcc);
    out.extend_from_slice(payload);
    out
}

fn full_box(fourcc: &[u8; 4], ver: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(4 + body.len());
    p.push(ver);
    p.extend_from_slice(&flags.to_be_bytes()[1..4]);
    p.extend_from_slice(body);
    mp4_box(fourcc, &p)
}

fn build_minimal_mp4() -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2avc1");
    let frame0 = b"\x00\x00\x00\x04\x65frame0".to_vec();
    let frame1 = b"\x00\x00\x00\x04\x41frame1".to_vec();
    let mdat_payload_offset = (ftyp.len() + 8) as u32;
    let mut mdat_body = Vec::new();
    mdat_body.extend_from_slice(&frame0);
    mdat_body.extend_from_slice(&frame1);
    let mdat = mp4_box(b"mdat", &mdat_body);

    // mvhd: version 0, timescale 1000, duration 80
    let mut mvhd_b = vec![0u8; 8];
    push_be_u32(&mut mvhd_b, 1000);
    push_be_u32(&mut mvhd_b, 80);
    mvhd_b.extend_from_slice(&[0u8; 76]);
    let mvhd = full_box(b"mvhd", 0, 0, &mvhd_b);

    // tkhd: version 0, id 1, width 320, height 180
    let mut tkhd_b = vec![0u8; 8];
    push_be_u32(&mut tkhd_b, 1);
    tkhd_b.extend_from_slice(&[0u8; 52]);
    push_be_u32(&mut tkhd_b, 320 << 16);
    push_be_u32(&mut tkhd_b, 180 << 16);
    let tkhd = full_box(b"tkhd", 0, 3, &tkhd_b);

    // edts -> elst: delay 40ms (40 units) on first sample media_time=40
    let mut elst_b = Vec::new();
    push_be_u32(&mut elst_b, 1);
    push_be_u32(&mut elst_b, 80); // seg dur in movie ts
    push_be_u32(&mut elst_b, 40); // media_time = 40 -> shift -40
    push_be_u32(&mut elst_b, 1 << 16);
    let elst = full_box(b"elst", 0, 0, &elst_b);
    let edts = mp4_box(b"edts", &elst);

    // mdhd: timescale 1000, dur 80
    let mut mdhd_b = vec![0u8; 8];
    push_be_u32(&mut mdhd_b, 1000);
    push_be_u32(&mut mdhd_b, 80);
    mdhd_b.extend_from_slice(&[0u8; 4]);
    let mdhd = full_box(b"mdhd", 0, 0, &mdhd_b);

    let mut hdlr_b = vec![0u8; 4];
    hdlr_b.extend_from_slice(b"vide");
    hdlr_b.extend_from_slice(&[0u8; 12]);
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr_b);

    // avc1 sample entry + avcC
    let avcc = mp4_box(b"avcC", b"\x01\x42\x00\x1e\xff");
    let mut avc1_payload = vec![0u8; 8]; // 6 reserved + 2 ref idx
    avc1_payload.extend_from_slice(&[0u8; 16]);
    push_be_u16(&mut avc1_payload, 320);
    push_be_u16(&mut avc1_payload, 180);
    avc1_payload.extend_from_slice(&[0u8; 50]);
    avc1_payload.extend_from_slice(&avcc);
    let avc1 = mp4_box(b"avc1", &avc1_payload);
    let mut stsd_b = Vec::new();
    push_be_u32(&mut stsd_b, 1);
    stsd_b.extend_from_slice(&avc1);
    let stsd = full_box(b"stsd", 0, 0, &stsd_b);

    let mut stts_b = Vec::new();
    push_be_u32(&mut stts_b, 1);
    push_be_u32(&mut stts_b, 2);
    push_be_u32(&mut stts_b, 40);
    let stts = full_box(b"stts", 0, 0, &stts_b);

    let mut ctts_b = Vec::new();
    push_be_u32(&mut ctts_b, 2);
    push_be_u32(&mut ctts_b, 1);
    push_be_u32(&mut ctts_b, 40);
    push_be_u32(&mut ctts_b, 1);
    push_be_u32(&mut ctts_b, 40);
    let ctts = full_box(b"ctts", 0, 0, &ctts_b);

    let mut stsc_b = Vec::new();
    push_be_u32(&mut stsc_b, 1);
    push_be_u32(&mut stsc_b, 1);
    push_be_u32(&mut stsc_b, 2);
    push_be_u32(&mut stsc_b, 1);
    let stsc = full_box(b"stsc", 0, 0, &stsc_b);

    let mut stsz_b = Vec::new();
    push_be_u32(&mut stsz_b, 0);
    push_be_u32(&mut stsz_b, 2);
    push_be_u32(&mut stsz_b, frame0.len() as u32);
    push_be_u32(&mut stsz_b, frame1.len() as u32);
    let stsz = full_box(b"stsz", 0, 0, &stsz_b);

    let mut stco_b = Vec::new();
    push_be_u32(&mut stco_b, 1);
    push_be_u32(&mut stco_b, mdat_payload_offset);
    let stco = full_box(b"stco", 0, 0, &stco_b);

    let mut stss_b = Vec::new();
    push_be_u32(&mut stss_b, 1);
    push_be_u32(&mut stss_b, 1);
    let stss = full_box(b"stss", 0, 0, &stss_b);

    let stbl = mp4_box(
        b"stbl",
        &[stsd, stts, ctts, stsc, stsz, stco, stss].concat(),
    );
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = mp4_box(b"trak", &[tkhd, edts, mdia].concat());
    let moov = mp4_box(b"moov", &[mvhd, trak].concat());

    [ftyp, mdat, moov].concat()
}

fn ebml_vint_size(n: u64) -> Vec<u8> {
    if n < 0x7f {
        vec![0x80 | (n as u8)]
    } else if n < 0x3fff {
        let v = 0x4000 | (n as u16);
        v.to_be_bytes().to_vec()
    } else {
        let v = 0x1000_0000 | (n as u32);
        v.to_be_bytes().to_vec()
    }
}

fn ebml_elem(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(id.len() + 4 + payload.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&ebml_vint_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn build_synthetic_mkv_with_all_lacings() -> Vec<u8> {
    let ebml = ebml_elem(
        &[0x1A, 0x45, 0xDF, 0xA3],
        &ebml_elem(&[0x42, 0x82], b"matroska"),
    );
    let ts_scale = ebml_elem(&[0x2A, 0xD7, 0xB1], &1_000_000u64.to_be_bytes()[4..8]);
    let info = ebml_elem(&[0x15, 0x49, 0xA9, 0x66], &ts_scale);

    // Track 1 (Video) + Track 2 (Audio)
    let t1 = ebml_elem(
        &[0xAE],
        &[
            ebml_elem(&[0xD7], &[1]),
            ebml_elem(&[0x83], &[1]),
            ebml_elem(&[0x86], b"V_VP9"),
            ebml_elem(&[0x23, 0xE3, 0x83], &40_000_000u64.to_be_bytes()),
            ebml_elem(
                &[0xE0],
                &[
                    ebml_elem(&[0xB0], &128u16.to_be_bytes()),
                    ebml_elem(&[0xBA], &72u16.to_be_bytes()),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let t2 = ebml_elem(
        &[0xAE],
        &[
            ebml_elem(&[0xD7], &[2]),
            ebml_elem(&[0x83], &[2]),
            ebml_elem(&[0x86], b"A_OPUS"),
            ebml_elem(&[0x63, 0xA2], b"OpusHead\x01\x02"),
            ebml_elem(&[0x56, 0xAA], &6_500_000u64.to_be_bytes()),
            ebml_elem(
                &[0xE1],
                &[
                    ebml_elem(&[0xB5], &48_000.0f32.to_be_bytes()),
                    ebml_elem(&[0x9F], &[2]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let tracks = ebml_elem(&[0x16, 0x54, 0xAE, 0x6B], &[t1, t2].concat());

    // SimpleBlock (no lace) para vídeo
    let mut sb_video = vec![0x81, 0x00, 0x00, 0x80];
    sb_video.extend_from_slice(b"VIDEOKEY");
    let e_video = ebml_elem(&[0xA3], &sb_video);

    // SimpleBlock Xiph lacing (3 quadros: tamanhos 3, 2, 4 -> count=2, xiph=[3,2], payloads)
    let mut sb_xiph = vec![0x82, 0x00, 0x00, 0x82, 2, 3, 2];
    sb_xiph.extend_from_slice(b"AAABB");
    sb_xiph.extend_from_slice(b"CCCC");
    let e_xiph = ebml_elem(&[0xA3], &sb_xiph);

    // BlockGroup Fixed-size lacing (2 quadros de 5 bytes -> count=1) + BlockDuration 40 + ReferenceBlock
    let mut blk_fixed = vec![0x81, 0x00, 0x28, 0x04, 1];
    blk_fixed.extend_from_slice(b"1111122222");
    let e_fixed = ebml_elem(
        &[0xA0],
        &[
            ebml_elem(&[0xA1], &blk_fixed),
            ebml_elem(&[0x9B], &[40]),
            ebml_elem(&[0xFB], &[0xEC]),
        ]
        .concat(),
    );

    // SimpleBlock EBML lacing (2 quadros: frame0=4 bytes (0x84), frame1=5 bytes)
    let mut sb_ebml = vec![0x82, 0x00, 0x14, 0x86, 1, 0x84];
    sb_ebml.extend_from_slice(b"XXXX");
    sb_ebml.extend_from_slice(b"YYYYY");
    let e_ebml = ebml_elem(&[0xA3], &sb_ebml);

    let cluster = ebml_elem(
        &[0x1F, 0x43, 0xB6, 0x75],
        &[ebml_elem(&[0xE7], &[0]), e_video, e_xiph, e_fixed, e_ebml].concat(),
    );
    let segment = ebml_elem(&[0x18, 0x53, 0x80, 0x67], &[info, tracks, cluster].concat());
    [ebml, segment].concat()
}

#[test]
fn mp4_parses_bframes_elst_avcc_and_stss() {
    let bytes = build_minimal_mp4();
    let idx = demux_bytes(&bytes).expect("demux MP4 sintético");
    assert_eq!(idx.container, ContainerKind::Mp4);
    assert_eq!(idx.tracks.len(), 1);
    let t = &idx.tracks[0];
    assert_eq!(t.kind, TrackKind::Video);
    assert_eq!(t.codec, "avc1");
    assert_eq!(t.extra, b"\x01\x42\x00\x1e\xff");
    assert_eq!((t.width, t.height), (320, 180));
    assert_eq!(idx.samples.len(), 2);

    let s0 = idx.samples[0];
    let s1 = idx.samples[1];
    assert!(s0.is_keyframe);
    assert!(!s1.is_keyframe);
    assert_eq!(
        &bytes[s0.offset as usize..][..s0.size as usize],
        b"\x00\x00\x00\x04\x65frame0"
    );
    assert_eq!(
        &bytes[s1.offset as usize..][..s1.size as usize],
        b"\x00\x00\x00\x04\x41frame1"
    );
    // edts shift (-40ms) + ctts (+40ms) -> s0.pts == 0, s0.dts == -40ms
    assert_eq!(s0.pts, Ticks::ZERO);
    assert_eq!(s0.dts, Ticks::from_millis_i64(-40));
    assert_eq!(s1.pts, Ticks::from_millis_i64(40));
}

#[test]
fn mkv_parses_all_three_lacings_block_group_and_codec_delay() {
    let bytes = build_synthetic_mkv_with_all_lacings();
    let idx = demux_bytes(&bytes).expect("demux MKV sintético");
    assert_eq!(idx.container, ContainerKind::Mkv);
    assert_eq!(idx.tracks.len(), 2);
    assert_eq!(idx.tracks[0].codec, "V_VP9");
    assert_eq!((idx.tracks[0].width, idx.tracks[0].height), (128, 72));
    assert_eq!(idx.tracks[1].codec, "A_OPUS");
    assert_eq!(idx.tracks[1].sample_rate, 48_000);
    assert_eq!(idx.tracks[1].channels, 2);

    // 1 video SimpleBlock + 3 Xiph audio + 2 Fixed BlockGroup video + 2 EBML audio = 8
    assert_eq!(idx.samples.len(), 8);
    assert!(idx.samples[0].is_keyframe);
    assert_eq!(
        &bytes[idx.samples[0].offset as usize..][..idx.samples[0].size as usize],
        b"VIDEOKEY"
    );
    // Xiph
    assert_eq!(
        &bytes[idx.samples[1].offset as usize..][..idx.samples[1].size as usize],
        b"AAA"
    );
    assert_eq!(
        &bytes[idx.samples[2].offset as usize..][..idx.samples[2].size as usize],
        b"BB"
    );
    assert_eq!(
        &bytes[idx.samples[3].offset as usize..][..idx.samples[3].size as usize],
        b"CCCC"
    );
    assert!(idx.samples[1].is_keyframe && idx.samples[2].is_keyframe && idx.samples[3].is_keyframe);
    // BlockGroup Fixed com ReferenceBlock -> não é keyframe
    assert!(!idx.samples[4].is_keyframe);
    assert_eq!(
        &bytes[idx.samples[4].offset as usize..][..idx.samples[4].size as usize],
        b"11111"
    );
    assert_eq!(
        &bytes[idx.samples[5].offset as usize..][..idx.samples[5].size as usize],
        b"22222"
    );
    // EBML lacing
    assert_eq!(
        &bytes[idx.samples[6].offset as usize..][..idx.samples[6].size as usize],
        b"XXXX"
    );
    assert_eq!(
        &bytes[idx.samples[7].offset as usize..][..idx.samples[7].size as usize],
        b"YYYYY"
    );
}

#[test]
fn hostile_containers_return_errors_without_panic() {
    // Arquivo muito curto / vazio
    assert!(demux_bytes(&[]).is_err());
    assert!(demux_bytes(b"ftyp").is_err());

    // Caixa MP4 excedendo EOF / size < 8 / largesize inválido
    let bad_mp4 = [0xff, 0xff, 0xff, 0xff, b'm', b'o', b'o', b'v', 0, 0, 0, 0];
    assert!(demux_bytes(&bad_mp4).is_err());

    let zero_scale_mp4 = {
        let mut b = build_minimal_mp4();
        // Corrompe mvhd timescale para 0
        if let Some(pos) = b.windows(4).position(|w| w == b"mvhd") {
            b[pos + 16..pos + 20].copy_from_slice(&0u32.to_be_bytes());
        }
        b
    };
    assert!(demux_bytes(&zero_scale_mp4).is_err());

    // EBML com VINT zero-width (0x00) ou size gigante
    let bad_mkv = [0x1A, 0x45, 0xDF, 0xA3, 0x00, 0x00, 0x00, 0x00];
    assert!(demux_bytes(&bad_mkv).is_err());

    // Mutação sistemática sob catch_unwind em MP4 e MKV
    for (label, seed_bytes) in [
        ("MP4 sintético", build_minimal_mp4()),
        ("MKV sintético", build_synthetic_mkv_with_all_lacings()),
    ] {
        for len in 0..=seed_bytes.len().min(64) {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = demux_bytes(&seed_bytes[..len]);
            }));
            assert!(r.is_ok(), "pânico no truncamento de {label} em {len}");
        }
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for step in 0..80 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mut mutated = seed_bytes.clone();
            let pos = (state as usize) % mutated.len();
            mutated[pos] ^= ((state >> 8) as u8).max(1);
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = demux_bytes(&mutated);
            }));
            assert!(r.is_ok(), "pânico na mutação {step} de {label}");
        }
    }
}

#[test]
fn co64_and_fragmented_moof_parse_correctly() {
    // Fragmentado simples (ftyp + moov/mvex + moof/traf/trun + mdat)
    let ftyp = mp4_box(b"ftyp", b"iso5\x00\x00\x00\x00iso5dash");
    let mut mvhd_b = vec![0u8; 8];
    push_be_u32(&mut mvhd_b, 1000);
    push_be_u32(&mut mvhd_b, 0);
    mvhd_b.extend_from_slice(&[0u8; 76]);
    let mvhd = full_box(b"mvhd", 0, 0, &mvhd_b);

    let mut tkhd_b = vec![0u8; 8];
    push_be_u32(&mut tkhd_b, 1);
    tkhd_b.extend_from_slice(&[0u8; 60]);
    let tkhd = full_box(b"tkhd", 0, 3, &tkhd_b);
    let mut mdhd_b = vec![0u8; 8];
    push_be_u32(&mut mdhd_b, 48_000);
    push_be_u32(&mut mdhd_b, 0);
    mdhd_b.extend_from_slice(&[0u8; 4]);
    let mdhd = full_box(b"mdhd", 0, 0, &mdhd_b);
    let mut hdlr_b = vec![0u8; 4];
    hdlr_b.extend_from_slice(b"soun");
    hdlr_b.extend_from_slice(&[0u8; 12]);
    let hdlr = full_box(b"hdlr", 0, 0, &hdlr_b);
    let mut stsd_b = Vec::new();
    push_be_u32(&mut stsd_b, 1);
    let mut mp4a_b = vec![0u8; 8]; // 6 reserved + 2 ref
    mp4a_b.extend_from_slice(&0u16.to_be_bytes());
    mp4a_b.extend_from_slice(&[0u8; 6]);
    push_be_u16(&mut mp4a_b, 2);
    push_be_u16(&mut mp4a_b, 16);
    mp4a_b.extend_from_slice(&[0u8; 4]);
    push_be_u32(&mut mp4a_b, 48_000 << 16);
    stsd_b.extend_from_slice(&mp4_box(b"mp4a", &mp4a_b));
    let stbl = mp4_box(b"stbl", &full_box(b"stsd", 0, 0, &stsd_b));
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, mp4_box(b"minf", &stbl)].concat());
    let trak = mp4_box(b"trak", &[tkhd, mdia].concat());

    let mut trex_b = Vec::new();
    push_be_u32(&mut trex_b, 1);
    push_be_u32(&mut trex_b, 1);
    push_be_u32(&mut trex_b, 1024);
    push_be_u32(&mut trex_b, 6);
    push_be_u32(&mut trex_b, 0);
    let mvex = mp4_box(b"mvex", &full_box(b"trex", 0, 0, &trex_b));
    let moov = mp4_box(b"moov", &[mvhd, trak, mvex].concat());

    // Constrói moof apontando data_offset para o payload do mdat logo após o moof
    let tfhd = {
        let mut b = Vec::new();
        push_be_u32(&mut b, 1);
        full_box(b"tfhd", 0, 0x02_0000, &b)
    };
    let tfdt = full_box(b"tfdt", 1, 0, &0u64.to_be_bytes());
    // Placeholder trun com data_offset presente
    let trun_body = |data_off: i32| {
        let mut b = Vec::new();
        push_be_u32(&mut b, 2); // 2 samples
        b.extend_from_slice(&data_off.to_be_bytes());
        full_box(b"trun", 0, 0x00_0001, &b)
    };
    let probe_moof = mp4_box(
        b"moof",
        &mp4_box(
            b"traf",
            &[tfhd.clone(), tfdt.clone(), trun_body(0)].concat(),
        ),
    );
    let data_offset = (probe_moof.len() + 8) as i32;
    let moof = mp4_box(
        b"moof",
        &mp4_box(b"traf", &[tfhd, tfdt, trun_body(data_offset)].concat()),
    );
    let mdat = mp4_box(b"mdat", b"AACPK1AACPK2");

    let mut full = [ftyp, moov, moof, mdat].concat();
    let idx = demux_bytes(&full).expect("demux fMP4");
    assert_eq!(idx.tracks[0].sample_rate, 48_000);
    assert_eq!(idx.tracks[0].channels, 2);
    assert_eq!(idx.samples.len(), 2);
    assert_eq!(&full[idx.samples[0].offset as usize..][..6], b"AACPK1");
    assert_eq!(&full[idx.samples[1].offset as usize..][..6], b"AACPK2");
    assert_eq!(idx.samples[1].pts, scale_to_ticks(1024, 1, 48_000).unwrap());

    // Testa também co64 modificando build_minimal_mp4 (stco -> co64)
    let base = build_minimal_mp4();
    if let Some(pos) = base.windows(4).position(|w| w == b"stco") {
        let sz_off = pos - 4;
        let orig_sz = u32::from_be_bytes([
            base[sz_off],
            base[sz_off + 1],
            base[sz_off + 2],
            base[sz_off + 3],
        ]) as usize;
        let off32 = u32::from_be_bytes([
            base[pos + 12],
            base[pos + 13],
            base[pos + 14],
            base[pos + 15],
        ]);
        let mut co64_b = Vec::new();
        push_be_u32(&mut co64_b, 1);
        push_be_u64(&mut co64_b, u64::from(off32));
        let co64 = full_box(b"co64", 0, 0, &co64_b);
        let delta = (co64.len() - orig_sz) as u32;
        full = Vec::with_capacity(base.len() + delta as usize);
        full.extend_from_slice(&base[..sz_off]);
        full.extend_from_slice(&co64);
        full.extend_from_slice(&base[sz_off + orig_sz..]);
        // Atualiza tamanhos das caixas pais (moov, trak, mdia, minf, stbl)
        for tag in [b"moov", b"trak", b"mdia", b"minf", b"stbl"] {
            let p = full.windows(4).position(|w| w == tag).unwrap() - 4;
            let s = u32::from_be_bytes([full[p], full[p + 1], full[p + 2], full[p + 3]]) + delta;
            full[p..p + 4].copy_from_slice(&s.to_be_bytes());
        }
        let idx64 = demux_bytes(&full).expect("demux MP4 com co64");
        assert_eq!(idx64.samples.len(), 2);
        assert_eq!(
            &full[idx64.samples[0].offset as usize..][..idx64.samples[0].size as usize],
            b"\x00\x00\x00\x04\x65frame0"
        );
    }
}

#[derive(serde::Deserialize)]
struct FfprobeOut {
    streams: Vec<FfprobeStream>,
    packets: Vec<FfprobePacket>,
}

#[derive(serde::Deserialize)]
struct FfprobeStream {
    index: usize,
    codec_type: String,
    codec_name: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    sample_rate: Option<String>,
    #[serde(default)]
    channels: Option<u16>,
}

#[derive(serde::Deserialize)]
struct FfprobePacket {
    stream_index: usize,
    pts_time: Option<String>,
    dts_time: Option<String>,
    size: String,
    pos: Option<String>,
    flags: String,
}

fn ffprobe_packets(path: &std::path::Path) -> Option<FfprobeOut> {
    let out = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-show_streams",
            "-show_packets",
            "-print_format",
            "json",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

#[test]
fn demux_fixtures_match_ffprobe_packets_oracle() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("fixtures");
    let files = [
        ("mf_smoke.mp4", ContainerKind::Mp4),
        ("oracle_aac.m4a", ContainerKind::Mp4),
        ("demux_frag.mp4", ContainerKind::Mp4),
        ("demux_bframes.mov", ContainerKind::Mov),
        ("demux_sample.mkv", ContainerKind::Mkv),
        ("demux_sample.webm", ContainerKind::WebM),
    ];
    for (name, expected_kind) in files {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        let Some(oracle) = ffprobe_packets(&path) else {
            eprintln!("ffprobe indisponível; oráculo pulado para {name}");
            return;
        };
        let idx = demux_file(&path).unwrap_or_else(|e| panic!("falha demux em {name}: {e}"));
        assert_eq!(idx.container, expected_kind, "{name}: contêiner");
        assert_eq!(
            idx.tracks.len(),
            oracle.streams.len(),
            "{name}: número de trilhas"
        );

        for (si, st) in oracle.streams.iter().enumerate() {
            let tr = &idx.tracks[st.index];
            match st.codec_type.as_str() {
                "video" => {
                    assert_eq!(tr.kind, TrackKind::Video, "{name} stream {si}");
                    assert_eq!((tr.width, tr.height), (st.width, st.height), "{name}");
                }
                "audio" => {
                    assert_eq!(tr.kind, TrackKind::Audio, "{name} stream {si}");
                    if let Some(sr_s) = &st.sample_rate {
                        if let Ok(sr) = sr_s.parse::<u32>() {
                            assert_eq!(tr.sample_rate, sr, "{name} sample_rate");
                        }
                    }
                    if let Some(ch) = st.channels {
                        assert_eq!(tr.channels, ch, "{name} channels");
                    }
                }
                _ => {}
            }
        }

        let is_isobmff = matches!(expected_kind, ContainerKind::Mp4 | ContainerKind::Mov);
        for si in 0..idx.tracks.len() {
            let got: Vec<&DemuxSample> = idx.samples_for_track(si).collect();
            let exp: Vec<&FfprobePacket> = oracle
                .packets
                .iter()
                .filter(|p| p.stream_index == si)
                .collect();
            assert_eq!(
                got.len(),
                exp.len(),
                "{name} trilha {si}: contagem de pacotes diverge ({} vs {})",
                got.len(),
                exp.len()
            );
            for (pi, (g, e)) in got.iter().zip(exp.iter()).enumerate() {
                let exp_sz: u32 = e.size.parse().unwrap();
                assert_eq!(g.size, exp_sz, "{name} trilha {si} pkt {pi}: size");
                assert_eq!(
                    g.is_keyframe,
                    e.flags.starts_with('K'),
                    "{name} trilha {si} pkt {pi}: keyframe"
                );
                if let Some(pos_s) = &e.pos {
                    let exp_pos: u64 = pos_s.parse().unwrap();
                    if is_isobmff {
                        assert_eq!(g.offset, exp_pos, "{name} trilha {si} pkt {pi}: offset");
                    } else {
                        assert!(
                            g.offset >= exp_pos && g.offset <= exp_pos + 16,
                            "{name} trilha {si} pkt {pi}: payload MKV offset ({}) fora da margem de pos ({exp_pos})",
                            g.offset
                        );
                    }
                }
                if let Some(pts_s) = &e.pts_time {
                    let exp_pts: f64 = pts_s.parse().unwrap();
                    let got_pts = g.pts.as_secs_f64();
                    assert!(
                        (got_pts - exp_pts).abs() <= 0.002,
                        "{name} trilha {si} pkt {pi}: PTS ({got_pts:.6}s) vs ffprobe ({exp_pts:.6}s)"
                    );
                }
                if let Some(dts_s) = &e.dts_time {
                    let exp_dts: f64 = dts_s.parse().unwrap();
                    let got_dts = g.dts.as_secs_f64();
                    assert!(
                        (got_dts - exp_dts).abs() <= 0.002,
                        "{name} trilha {si} pkt {pi}: DTS ({got_dts:.6}s) vs ffprobe ({exp_dts:.6}s)"
                    );
                }
                let _ = &oracle.streams[si].codec_name;
            }
        }
    }
}
