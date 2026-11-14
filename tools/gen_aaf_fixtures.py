"""Gera fixtures AAF para os testes do importador (AGENTS.md §2.3 e §3.4).

Uso: python tools/gen_aaf_fixtures.py <pasta_de_saida> [pasta_pyaaf2]

Usa o pacote externo `pyaaf2` (MIT) apenas como gerador de fixtures, como o
ffmpeg para mídia: nada dele entra no Neoveg nem no repositório. As fixtures
vão para `target/fixtures/` e não são commitadas.

Arquivos gerados (25 fps):
  oracle_basic.aaf
    vídeo: A [in 10, 60 q] | filler 10 q | B [in 0, 70 q]
    áudio: A [in 10, 60 q]
  oracle_transition.aaf
    vídeo: A [in 10, 60 q] | dissolução 20 q (corte em 10) | B [in 0, 70 q]
"""

import os
import sys

RATE = 25
# Identificador público do efeito de dissolução de vídeo do AAF.
VIDEO_DISSOLVE = "0c3bea40-fc05-11d2-8a29-0050040ef7d2"


def build_masters(f):
    def master(name, url, length):
        smob = f.create.SourceMob(name + " fonte")
        desc = f.create.ImportDescriptor()
        loc = f.create.NetworkLocator()
        loc["URLString"].value = url
        desc["Locator"].append(loc)
        smob.descriptor = desc
        f.content.mobs.append(smob)
        vslot = smob.create_empty_slot(RATE, media_kind="picture", slot_id=1)
        vslot.segment.length = length
        aslot = smob.create_empty_slot(RATE, media_kind="sound", slot_id=2)
        aslot.segment.length = length
        mmob = f.create.MasterMob(name)
        f.content.mobs.append(mmob)
        mv = mmob.create_timeline_slot(RATE, slot_id=1)
        mv.segment = smob.create_source_clip(1, 0, length)
        ma = mmob.create_timeline_slot(RATE, slot_id=2)
        ma.segment = smob.create_source_clip(2, 0, length)
        return mmob

    a = master("A", "file:///C:/midia/a%20b.mov", 300)
    b = master("B", "file:///C:/midia/b.mov", 300)
    return a, b


def write_basic(aaf2, path):
    with aaf2.open(path, "w") as f:
        a, b = build_masters(f)
        comp = f.create.CompositionMob("Corte AAF")
        f.content.mobs.append(comp)
        vseq = f.create.Sequence(media_kind="picture")
        comp.create_timeline_slot(RATE, slot_id=1).segment = vseq
        vseq.components.append(a.create_source_clip(1, 10, 60))
        vseq.components.append(f.create.Filler(media_kind="picture", length=10))
        vseq.components.append(b.create_source_clip(1, 0, 70))
        aseq = f.create.Sequence(media_kind="sound")
        comp.create_timeline_slot(RATE, slot_id=2).segment = aseq
        aseq.components.append(a.create_source_clip(2, 10, 60))


def write_transition(aaf2, path):
    with aaf2.open(path, "w") as f:
        a, b = build_masters(f)
        comp = f.create.CompositionMob("Transição AAF")
        f.content.mobs.append(comp)
        vseq = f.create.Sequence(media_kind="picture")
        comp.create_timeline_slot(RATE, slot_id=1).segment = vseq
        vseq.components.append(a.create_source_clip(1, 10, 60))

        opdef = f.create.OperationDef(VIDEO_DISSOLVE, "Video Dissolve", "")
        opdef.media_kind = "picture"
        opdef["NumberInputs"].value = 2
        f.dictionary.register_def(opdef)
        og = f.create.OperationGroup(opdef, length=20)
        og.media_kind = "picture"
        tr = f.create.Transition(media_kind="picture", length=20)
        tr["OperationGroup"].value = og
        tr["CutPoint"].value = 10
        vseq.components.append(tr)
        vseq.components.append(b.create_source_clip(1, 0, 70))


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    out_dir = sys.argv[1]
    if len(sys.argv) > 2:
        sys.path.insert(0, sys.argv[2])
    try:
        import aaf2  # noqa: PLC0415
    except ImportError:
        print("pyaaf2 não encontrado", file=sys.stderr)
        return 3
    os.makedirs(out_dir, exist_ok=True)

    status = 0
    for name, fn in (
        ("oracle_basic.aaf", write_basic),
        ("oracle_transition.aaf", write_transition),
    ):
        path = os.path.join(out_dir, name)
        try:
            fn(aaf2, path)
            print("ok", path)
        except Exception as e:  # noqa: BLE001 - relata e segue para o próximo
            print("falhou", name, type(e).__name__, e, file=sys.stderr)
            status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
