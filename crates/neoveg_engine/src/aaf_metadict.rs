// Gerado de forma determinística a partir dos registros públicos SMPTE:
// - SMPTE ST 395 (Groups / Classes, Reg-XML 13/1 AAF)
// - SMPTE ST 335 (Elements / Properties e LocalIdentification PIDs)
// - SMPTE ST 2003 (Types)
// - AMWA MS-01 v1.1 §5 / §27 (MetaDictionary) e MS-02 v1.0.1

pub(crate) const fn auid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let a = d1.to_le_bytes();
    let b = d2.to_le_bytes();
    let c = d3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5],
        d4[6], d4[7],
    ]
}

pub(crate) struct MetaPropDef {
    pub(crate) pid: u16,
    pub(crate) name: &'static str,
    pub(crate) id: [u8; 16],
    pub(crate) typ: [u8; 16],
    pub(crate) optional: bool,
    pub(crate) unique: bool,
}

pub(crate) struct MetaClassDef {
    pub(crate) name: &'static str,
    pub(crate) id: [u8; 16],
    pub(crate) parent: [u8; 16],
    pub(crate) concrete: bool,
    pub(crate) props: &'static [MetaPropDef],
}

pub(crate) enum MetaTypeKind {
    Integer {
        size: u8,
        signed: bool,
    },
    Character,
    String {
        elem: [u8; 16],
    },
    Rename {
        target: [u8; 16],
    },
    StrongRef {
        target_cls: [u8; 16],
    },
    WeakRef {
        target_cls: [u8; 16],
        target_set: &'static [[u8; 16]],
    },
    Enum {
        elem: [u8; 16],
        names: &'static [&'static str],
        values: &'static [i64],
    },
    FixedArray {
        elem: [u8; 16],
        count: u32,
    },
    VarArray {
        elem: [u8; 16],
    },
    Set {
        elem: [u8; 16],
    },
    Record {
        names: &'static [&'static str],
        types: &'static [[u8; 16]],
    },
}

pub(crate) struct MetaTypeDef {
    pub(crate) class_id: [u8; 16],
    pub(crate) name: &'static str,
    pub(crate) id: [u8; 16],
    pub(crate) kind: MetaTypeKind,
}

pub(crate) static META_CLASSES: &[MetaClassDef] = &[
    MetaClassDef {
        name: "Root",
        id: auid(
            0x0D010101,
            0x0300,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0300,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0001,
                name: "MetaDictionary",
                id: auid(
                    0x06010107,
                    0x1600,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                typ: auid(
                    0x05022700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0002,
                name: "Header",
                id: auid(
                    0x06010107,
                    0x1700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                typ: auid(
                    0x05022800,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "InterchangeObject",
        id: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[MetaPropDef {
            pid: 0x0101,
            name: "ObjClass",
            id: auid(
                0x06010104,
                0x0101,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: true,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "Component",
        id: auid(
            0x0D010101,
            0x0101,
            0x0200,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x0201,
                name: "DataDefinition",
                id: auid(
                    0x04070100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0202,
                name: "Length",
                id: auid(
                    0x07020201,
                    0x0103,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01012002,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "Segment",
        id: auid(
            0x0D010101,
            0x0101,
            0x0300,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0200,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[],
    },
    MetaClassDef {
        name: "Filler",
        id: auid(
            0x0D010101,
            0x0101,
            0x0900,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0300,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "OperationGroup",
        id: auid(
            0x0D010101,
            0x0101,
            0x0A00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0300,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x0B01,
            name: "Operation",
            id: auid(
                0x05300506,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010700,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "Sequence",
        id: auid(
            0x0D010101,
            0x0101,
            0x0F00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0300,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x1001,
            name: "Components",
            id: auid(
                0x06010104,
                0x0609,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05060100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "SourceReference",
        id: auid(
            0x0D010101,
            0x0101,
            0x1000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0300,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x1101,
                name: "SourceID",
                id: auid(
                    0x06010103,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x1102,
                name: "SourceMobSlotID",
                id: auid(
                    0x06010103,
                    0x0200,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "SourceClip",
        id: auid(
            0x0D010101,
            0x0101,
            0x1100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x1000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x1201,
            name: "StartTime",
            id: auid(
                0x07020103,
                0x0104,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x01012001,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: true,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "Transition",
        id: auid(
            0x0D010101,
            0x0101,
            0x1700,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0200,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x1802,
                name: "CutPoint",
                id: auid(
                    0x07020103,
                    0x0106,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01012001,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x1801,
                name: "OperationGroup",
                id: auid(
                    0x06010104,
                    0x0205,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05020500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "ContentStorage",
        id: auid(
            0x0D010101,
            0x0101,
            0x1800,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x1901,
            name: "Mobs",
            id: auid(
                0x06010104,
                0x0501,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05050700,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "DefinitionObject",
        id: auid(
            0x0D010101,
            0x0101,
            0x1A00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x1B03,
                name: "Description",
                id: auid(
                    0x03020301,
                    0x0201,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x1B02,
                name: "Name",
                id: auid(
                    0x01070102,
                    0x0301,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x1B01,
                name: "Identification",
                id: auid(
                    0x01011503,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: true,
            },
        ],
    },
    MetaClassDef {
        name: "DataDefinition",
        id: auid(
            0x0D010101,
            0x0101,
            0x1B00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x1A00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "OperationDefinition",
        id: auid(
            0x0D010101,
            0x0101,
            0x1C00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x1A00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x1E07,
                name: "NumberInputs",
                id: auid(
                    0x05300504,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x01],
                ),
                typ: auid(
                    0x01010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x1E01,
                name: "DataDefinition",
                id: auid(
                    0x05300509,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "Dictionary",
        id: auid(
            0x0D010101,
            0x0101,
            0x2200,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x2603,
                name: "OperationDefinitions",
                id: auid(
                    0x06010104,
                    0x0503,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05050800,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x2605,
                name: "DataDefinitions",
                id: auid(
                    0x06010104,
                    0x0505,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05050400,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "EssenceDescriptor",
        id: auid(
            0x0D010101,
            0x0101,
            0x2400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[MetaPropDef {
            pid: 0x2F01,
            name: "Locator",
            id: auid(
                0x06010104,
                0x0603,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05060400,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: true,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "Header",
        id: auid(
            0x0D010101,
            0x0101,
            0x2F00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x3B01,
                name: "ByteOrder",
                id: auid(
                    0x03010201,
                    0x0200,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x01],
                ),
                typ: auid(
                    0x01010600,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B07,
                name: "ObjectModelVersion",
                id: auid(
                    0x03010201,
                    0x0400,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B05,
                name: "Version",
                id: auid(
                    0x03010201,
                    0x0500,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B09,
                name: "OperationalPattern",
                id: auid(
                    0x01020203,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x05],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B03,
                name: "Content",
                id: auid(
                    0x06010104,
                    0x0201,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05020100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B04,
                name: "Dictionary",
                id: auid(
                    0x06010104,
                    0x0202,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05020200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B06,
                name: "IdentificationList",
                id: auid(
                    0x06010104,
                    0x0604,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05060300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3B02,
                name: "LastModified",
                id: auid(
                    0x07020110,
                    0x0204,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "Identification",
        id: auid(
            0x0D010101,
            0x0101,
            0x3000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x3C09,
                name: "GenerationAUID",
                id: auid(
                    0x05200701,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C05,
                name: "ProductID",
                id: auid(
                    0x05200701,
                    0x0700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C01,
                name: "CompanyName",
                id: auid(
                    0x05200701,
                    0x0201,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C02,
                name: "ProductName",
                id: auid(
                    0x05200701,
                    0x0301,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C04,
                name: "ProductVersionString",
                id: auid(
                    0x05200701,
                    0x0501,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C08,
                name: "Platform",
                id: auid(
                    0x05200701,
                    0x0601,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x3C06,
                name: "Date",
                id: auid(
                    0x07020110,
                    0x0203,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "Locator",
        id: auid(
            0x0D010101,
            0x0101,
            0x3100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[],
    },
    MetaClassDef {
        name: "NetworkLocator",
        id: auid(
            0x0D010101,
            0x0101,
            0x3200,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x3100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x4001,
            name: "URLString",
            id: auid(
                0x01020101,
                0x0100,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x01],
            ),
            typ: auid(
                0x01100200,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "Mob",
        id: auid(
            0x0D010101,
            0x0101,
            0x3400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x4402,
                name: "Name",
                id: auid(
                    0x01030302,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x01],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4403,
                name: "Slots",
                id: auid(
                    0x06010104,
                    0x0605,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05060500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4405,
                name: "CreationTime",
                id: auid(
                    0x07020110,
                    0x0103,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4404,
                name: "LastModified",
                id: auid(
                    0x07020110,
                    0x0205,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4401,
                name: "MobID",
                id: auid(
                    0x01011510,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x01],
                ),
                typ: auid(
                    0x01030200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: true,
            },
        ],
    },
    MetaClassDef {
        name: "CompositionMob",
        id: auid(
            0x0D010101,
            0x0101,
            0x3500,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x3400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "MasterMob",
        id: auid(
            0x0D010101,
            0x0101,
            0x3600,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x3400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "SourceMob",
        id: auid(
            0x0D010101,
            0x0101,
            0x3700,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x3400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x4701,
            name: "EssenceDescription",
            id: auid(
                0x06010104,
                0x0203,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05020300,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "MobSlot",
        id: auid(
            0x0D010101,
            0x0101,
            0x3800,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x0100,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x4801,
                name: "SlotID",
                id: auid(
                    0x01070101,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4802,
                name: "SlotName",
                id: auid(
                    0x01070102,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4804,
                name: "PhysicalTrackNumber",
                id: auid(
                    0x01040103,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4803,
                name: "Segment",
                id: auid(
                    0x06010104,
                    0x0204,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05020600,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TimelineMobSlot",
        id: auid(
            0x0D010101,
            0x0101,
            0x3B00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x3800,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x4B02,
                name: "Origin",
                id: auid(
                    0x07020103,
                    0x0103,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01012001,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x4B01,
                name: "EditRate",
                id: auid(
                    0x05300405,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x03010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "PhysicalDescriptor",
        id: auid(
            0x0D010101,
            0x0101,
            0x4900,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x2400,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[],
    },
    MetaClassDef {
        name: "ImportDescriptor",
        id: auid(
            0x0D010101,
            0x0101,
            0x4A00,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0101,
            0x4900,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "ClassDefinition",
        id: auid(
            0x0D010101,
            0x0201,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0224,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0008,
                name: "ParentClass",
                id: auid(
                    0x06010107,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0009,
                name: "Properties",
                id: auid(
                    0x06010107,
                    0x0200,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05050B00,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x000A,
                name: "IsConcrete",
                id: auid(
                    0x06010107,
                    0x0300,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01040100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "PropertyDefinition",
        id: auid(
            0x0D010101,
            0x0202,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0224,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x000C,
                name: "IsOptional",
                id: auid(
                    0x03010202,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01040100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x000B,
                name: "Type",
                id: auid(
                    0x06010107,
                    0x0400,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x000D,
                name: "LocalIdentification",
                id: auid(
                    0x06010107,
                    0x0500,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x000E,
                name: "IsUniqueIdentifier",
                id: auid(
                    0x06010107,
                    0x0600,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01040100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinition",
        id: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0224,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[],
    },
    MetaClassDef {
        name: "TypeDefinitionInteger",
        id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x000F,
                name: "Size",
                id: auid(
                    0x03010203,
                    0x0100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0010,
                name: "IsSigned",
                id: auid(
                    0x03010203,
                    0x0200,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01040100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinitionStrongObjectReference",
        id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x0011,
            name: "ReferencedType",
            id: auid(
                0x06010107,
                0x0900,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "TypeDefinitionWeakObjectReference",
        id: auid(
            0x0D010101,
            0x0206,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0013,
                name: "TargetSet",
                id: auid(
                    0x03010203,
                    0x0B00,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x04010600,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0012,
                name: "ReferencedType",
                id: auid(
                    0x06010107,
                    0x0A00,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinitionEnumeration",
        id: auid(
            0x0D010101,
            0x0207,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0015,
                name: "ElementNames",
                id: auid(
                    0x03010203,
                    0x0400,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x04010500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0016,
                name: "ElementValues",
                id: auid(
                    0x03010203,
                    0x0500,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x04010400,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0014,
                name: "ElementType",
                id: auid(
                    0x06010107,
                    0x0B00,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010900,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinitionFixedArray",
        id: auid(
            0x0D010101,
            0x0208,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0018,
                name: "ElementCount",
                id: auid(
                    0x03010203,
                    0x0300,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0017,
                name: "ElementType",
                id: auid(
                    0x06010107,
                    0x0C00,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05010900,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinitionVariableArray",
        id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x0019,
            name: "ElementType",
            id: auid(
                0x06010107,
                0x0D00,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "TypeDefinitionSet",
        id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x001A,
            name: "ElementType",
            id: auid(
                0x06010107,
                0x0E00,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "TypeDefinitionString",
        id: auid(
            0x0D010101,
            0x020B,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x001B,
            name: "ElementType",
            id: auid(
                0x06010107,
                0x0F00,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "TypeDefinitionRecord",
        id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x001D,
                name: "MemberNames",
                id: auid(
                    0x03010203,
                    0x0600,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x04010500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x001C,
                name: "MemberTypes",
                id: auid(
                    0x06010107,
                    0x1100,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05040200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "TypeDefinitionRename",
        id: auid(
            0x0D010101,
            0x020E,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[MetaPropDef {
            pid: 0x001E,
            name: "RenamedType",
            id: auid(
                0x06010107,
                0x1200,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
            ),
            typ: auid(
                0x05010900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            optional: false,
            unique: false,
        }],
    },
    MetaClassDef {
        name: "TypeDefinitionCharacter",
        id: auid(
            0x0D010101,
            0x0223,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0203,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[],
    },
    MetaClassDef {
        name: "MetaDefinition",
        id: auid(
            0x0D010101,
            0x0224,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0224,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: false,
        props: &[
            MetaPropDef {
                pid: 0x0006,
                name: "Name",
                id: auid(
                    0x03020401,
                    0x0201,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0005,
                name: "Identification",
                id: auid(
                    0x06010107,
                    0x1300,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: false,
                unique: true,
            },
            MetaPropDef {
                pid: 0x0007,
                name: "Description",
                id: auid(
                    0x06010107,
                    0x1401,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x01100200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
    MetaClassDef {
        name: "MetaDictionary",
        id: auid(
            0x0D010101,
            0x0225,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        parent: auid(
            0x0D010101,
            0x0225,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        concrete: true,
        props: &[
            MetaPropDef {
                pid: 0x0003,
                name: "ClassDefinitions",
                id: auid(
                    0x06010107,
                    0x0700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05050100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
            MetaPropDef {
                pid: 0x0004,
                name: "TypeDefinitions",
                id: auid(
                    0x06010107,
                    0x0800,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                typ: auid(
                    0x05050C00,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                optional: true,
                unique: false,
            },
        ],
    },
];

pub(crate) static META_TYPES: &[MetaTypeDef] = &[
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UInt8",
        id: auid(
            0x01010100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 1,
            signed: false,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Rational",
        id: auid(
            0x03010100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["Numerator", "Denominator"],
            types: &[
                auid(
                    0x01010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010700,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0206,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ClassDefinitionWeakReference",
        id: auid(
            0x05010100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::WeakRef {
            target_cls: auid(
                0x0D010101,
                0x0201,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
            target_set: &[
                auid(
                    0x06010107,
                    0x1600,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                auid(
                    0x06010107,
                    0x0700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ContentStorageStrongReference",
        id: auid(
            0x05020100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x1800,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "AUID",
        id: auid(
            0x01030100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["Data1", "Data2", "Data3", "Data4"],
            types: &[
                auid(
                    0x01010300,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x04010800,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0207,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Boolean",
        id: auid(
            0x01040100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Enum {
            elem: auid(
                0x01010100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            names: &["False", "True"],
            values: &[0, 1],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ClassDefinitionStrongReferenceSet",
        id: auid(
            0x05050100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05020900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ComponentStrongReferenceVector",
        id: auid(
            0x05060100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x05020B00,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0223,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Character",
        id: auid(
            0x01100100,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Character,
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UInt16",
        id: auid(
            0x01010200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 2,
            signed: false,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0208,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UInt8Array12",
        id: auid(
            0x04010200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::FixedArray {
            elem: auid(
                0x01010100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            count: 12,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "DictionaryStrongReference",
        id: auid(
            0x05020200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x2200,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PackageIDType",
        id: auid(
            0x01030200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &[
                "SMPTELabel",
                "Length",
                "InstanceHigh",
                "InstanceMid",
                "InstanceLow",
                "Material",
            ],
            types: &[
                auid(
                    0x04010200,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01030100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TypeDefinitionWeakReferenceVector",
        id: auid(
            0x05040200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x05010900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020B,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UTF16String",
        id: auid(
            0x01100200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::String {
            elem: auid(
                0x01100100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UInt32",
        id: auid(
            0x01010300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 4,
            signed: false,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "VersionType",
        id: auid(
            0x03010300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["VersionMajor", "VersionMinor"],
            types: &[
                auid(
                    0x01010500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0206,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "DataDefinitionWeakReference",
        id: auid(
            0x05010300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::WeakRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x1B00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
            target_set: &[
                auid(
                    0x06010107,
                    0x1700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                auid(
                    0x06010104,
                    0x0202,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                auid(
                    0x06010104,
                    0x0505,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "EssenceDescriptorStrongReference",
        id: auid(
            0x05020300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x2400,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "IdentificationStrongReferenceVector",
        id: auid(
            0x05060300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x05021000,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Int64Array",
        id: auid(
            0x04010400,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x01010800,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "DataDefinitionStrongReferenceSet",
        id: auid(
            0x05050400,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05020E00,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "LocatorStrongReferenceVector",
        id: auid(
            0x05060400,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x05021200,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Int8",
        id: auid(
            0x01010500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 1,
            signed: true,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "DateStruct",
        id: auid(
            0x03010500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["Year", "Month", "Day"],
            types: &[
                auid(
                    0x01010600,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UTF16StringArray",
        id: auid(
            0x04010500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x01100100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "OperationGroupStrongReference",
        id: auid(
            0x05020500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x0A00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TrackStrongReferenceVector",
        id: auid(
            0x05060500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x05021400,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Int16",
        id: auid(
            0x01010600,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 2,
            signed: true,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TimeStruct",
        id: auid(
            0x03010600,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["Hour", "Minute", "Second", "Fraction"],
            types: &[
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x01010100,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0209,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "AUIDArray",
        id: auid(
            0x04010600,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::VarArray {
            elem: auid(
                0x01030100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "SegmentStrongReference",
        id: auid(
            0x05020600,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x0300,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Int32",
        id: auid(
            0x01010700,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 4,
            signed: true,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020D,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TimeStamp",
        id: auid(
            0x03010700,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Record {
            names: &["Date", "Time"],
            types: &[
                auid(
                    0x03010500,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
                auid(
                    0x03010600,
                    0x0000,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0206,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "OperationDefinitionWeakReference",
        id: auid(
            0x05010700,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::WeakRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x1C00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
            target_set: &[
                auid(
                    0x06010107,
                    0x1700,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                auid(
                    0x06010104,
                    0x0202,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
                auid(
                    0x06010104,
                    0x0503,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PackageStrongReferenceSet",
        id: auid(
            0x05050700,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05021300,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0204,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "Int64",
        id: auid(
            0x01010800,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Integer {
            size: 8,
            signed: true,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0208,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "UInt8Array8",
        id: auid(
            0x04010800,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::FixedArray {
            elem: auid(
                0x01010100,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
            count: 8,
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "OperationDefinitionStrongReferenceSet",
        id: auid(
            0x05050800,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05021500,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0206,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TypeDefinitionWeakReference",
        id: auid(
            0x05010900,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::WeakRef {
            target_cls: auid(
                0x0D010101,
                0x0203,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
            target_set: &[
                auid(
                    0x06010107,
                    0x1600,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x0A],
                ),
                auid(
                    0x06010107,
                    0x0800,
                    0x0000,
                    [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x02],
                ),
            ],
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ClassDefinitionStrongReference",
        id: auid(
            0x05020900,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0201,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "ComponentStrongReference",
        id: auid(
            0x05020B00,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x0200,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PropertyDefinitionStrongReferenceSet",
        id: auid(
            0x05050B00,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05021900,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020A,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TypeDefinitionStrongReferenceSet",
        id: auid(
            0x05050C00,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Set {
            elem: auid(
                0x05021B00,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "DataDefinitionStrongReference",
        id: auid(
            0x05020E00,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x1B00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "IdentificationStrongReference",
        id: auid(
            0x05021000,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x3000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "LocatorStrongReference",
        id: auid(
            0x05021200,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x3100,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PackageStrongReference",
        id: auid(
            0x05021300,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x3400,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TrackStrongReference",
        id: auid(
            0x05021400,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x3800,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "OperationDefinitionStrongReference",
        id: auid(
            0x05021500,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x1C00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PropertyDefinitionStrongReference",
        id: auid(
            0x05021900,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0202,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "TypeDefinitionStrongReference",
        id: auid(
            0x05021B00,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0203,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "MetaDictionaryStrongReference",
        id: auid(
            0x05022700,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0225,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x0205,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PrefaceStrongReference",
        id: auid(
            0x05022800,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::StrongRef {
            target_cls: auid(
                0x0D010101,
                0x0101,
                0x2F00,
                [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020E,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "PositionType",
        id: auid(
            0x01012001,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Rename {
            target: auid(
                0x01010800,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
    MetaTypeDef {
        class_id: auid(
            0x0D010101,
            0x020E,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
        ),
        name: "LengthType",
        id: auid(
            0x01012002,
            0x0000,
            0x0000,
            [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
        ),
        kind: MetaTypeKind::Rename {
            target: auid(
                0x01010800,
                0x0000,
                0x0000,
                [0x06, 0x0E, 0x2B, 0x34, 0x01, 0x04, 0x01, 0x01],
            ),
        },
    },
];

use crate::aaf::{
    create_object_clsid, encode_utf16le_nul, weak_ref_auid, write_properties, write_set_index,
    write_stream, SetIndexEntry, CLASS_CLASS_DEFINITION, CLASS_METADICT, CLASS_PROPERTY_DEFINITION,
    PID_METADEF_IDENT, REF_IDX_META_CLASSES, REF_IDX_META_TYPES, SF_DATA, SF_STRONG_REF_SET,
    SF_WEAK_REF, SF_WEAK_REF_VECTOR,
};
use neoveg_common::error::NeovegError;
use std::io::{Read, Seek, Write};
use uuid::Uuid;

const PID_METADICT_CLASSES: u16 = 0x0003;
const PID_METADICT_TYPES: u16 = 0x0004;
const PID_METADEF_NAME: u16 = 0x0006;

const PID_CLASSDEF_PARENT: u16 = 0x0008;
const PID_CLASSDEF_PROPERTIES: u16 = 0x0009;
const PID_CLASSDEF_IS_CONCRETE: u16 = 0x000A;

const PID_PROPDEF_TYPE: u16 = 0x000B;
const PID_PROPDEF_IS_OPTIONAL: u16 = 0x000C;
const PID_PROPDEF_LOCAL_ID: u16 = 0x000D;
const PID_PROPDEF_IS_UNIQUE: u16 = 0x000E;

const PID_INT_SIZE: u16 = 0x000F;
const PID_INT_IS_SIGNED: u16 = 0x0010;
const PID_STRONGREF_TYPE: u16 = 0x0011;
const PID_WEAKREF_TYPE: u16 = 0x0012;
const PID_WEAKREF_TARGET_SET: u16 = 0x0013;
const PID_ENUM_ELEMENT_TYPE: u16 = 0x0014;
const PID_ENUM_ELEMENT_NAMES: u16 = 0x0015;
const PID_ENUM_ELEMENT_VALUES: u16 = 0x0016;
const PID_FIXEDARRAY_ELEM_TYPE: u16 = 0x0017;
const PID_FIXEDARRAY_ELEM_COUNT: u16 = 0x0018;
const PID_VARARRAY_ELEM_TYPE: u16 = 0x0019;
const PID_SET_ELEM_TYPE: u16 = 0x001A;
const PID_STRING_ELEM_TYPE: u16 = 0x001B;
const PID_RECORD_MEMBER_TYPES: u16 = 0x001C;
const PID_RECORD_MEMBER_NAMES: u16 = 0x001D;
const PID_RENAME_TYPE: u16 = 0x001E;

fn pack_utf16le_strings(items: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in items {
        out.extend_from_slice(&encode_utf16le_nul(s));
    }
    out
}

fn write_weak_ref_vector_index<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    parent_path: &str,
    list_name: &str,
    ref_idx: u16,
    ident_pid: u16,
    keys: &[[u8; 16]],
) -> Result<(), NeovegError> {
    let n = u32::try_from(keys.len()).map_err(|_| {
        NeovegError::InvalidInput("vetor de referências fracas AAF excedeu limite".into())
    })?;
    let mut buf = Vec::with_capacity(9 + keys.len() * 16);
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&ref_idx.to_le_bytes());
    buf.extend_from_slice(&ident_pid.to_le_bytes());
    buf.push(16);
    for k in keys {
        buf.extend_from_slice(k);
    }
    let path = format!("{}/{list_name} index", parent_path.trim_end_matches('/'));
    write_stream(comp, &path, &buf)
}

/// Grava `/MetaDictionary-1` completo conforme MS-01 §5 / §27.20 para o modelo de
/// classes e tipos utilizado na exportação AAF.
pub(crate) fn write_meta_dictionary<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
) -> Result<(), NeovegError> {
    let md = "/MetaDictionary-1";
    comp.create_storage(md).map_err(|e| {
        NeovegError::InvalidInput(format!("falha ao criar storage AAF '{md}': {e}"))
    })?;
    comp.set_storage_clsid(md, Uuid::from_bytes_le(CLASS_METADICT))
        .map_err(|e| NeovegError::InvalidInput(format!("falha ao definir CLSID de '{md}': {e}")))?;
    write_properties(
        comp,
        md,
        &[
            (
                PID_METADICT_CLASSES,
                SF_STRONG_REF_SET,
                encode_utf16le_nul("ClassDefinitions-3"),
            ),
            (
                PID_METADICT_TYPES,
                SF_STRONG_REF_SET,
                encode_utf16le_nul("TypeDefinitions-4"),
            ),
        ],
    )?;

    let mut cls_entries: Vec<SetIndexEntry<'_>> = Vec::with_capacity(META_CLASSES.len());
    for (ci, mc) in META_CLASSES.iter().enumerate() {
        let cls_path = format!("{md}/ClassDefinitions-3{{{ci:x}}}");
        create_object_clsid(
            comp,
            &cls_path,
            Uuid::from_bytes_le(CLASS_CLASS_DEFINITION),
            &[
                (PID_METADEF_IDENT, SF_DATA, mc.id.to_vec()),
                (PID_METADEF_NAME, SF_DATA, encode_utf16le_nul(mc.name)),
                (
                    PID_CLASSDEF_PARENT,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_CLASSES, PID_METADEF_IDENT, &mc.parent),
                ),
                (
                    PID_CLASSDEF_IS_CONCRETE,
                    SF_DATA,
                    vec![u8::from(mc.concrete)],
                ),
                (
                    PID_CLASSDEF_PROPERTIES,
                    SF_STRONG_REF_SET,
                    encode_utf16le_nul("Properties-9"),
                ),
            ],
        )?;

        let mut prop_entries: Vec<SetIndexEntry<'_>> = Vec::with_capacity(mc.props.len());
        for (pi, pd) in mc.props.iter().enumerate() {
            let pp = format!("{cls_path}/Properties-9{{{pi:x}}}");
            create_object_clsid(
                comp,
                &pp,
                Uuid::from_bytes_le(CLASS_PROPERTY_DEFINITION),
                &[
                    (PID_METADEF_IDENT, SF_DATA, pd.id.to_vec()),
                    (PID_METADEF_NAME, SF_DATA, encode_utf16le_nul(pd.name)),
                    (PID_PROPDEF_TYPE, SF_DATA, pd.typ.to_vec()),
                    (
                        PID_PROPDEF_IS_OPTIONAL,
                        SF_DATA,
                        vec![u8::from(pd.optional)],
                    ),
                    (PID_PROPDEF_LOCAL_ID, SF_DATA, pd.pid.to_le_bytes().to_vec()),
                    (PID_PROPDEF_IS_UNIQUE, SF_DATA, vec![u8::from(pd.unique)]),
                ],
            )?;
            prop_entries.push(SetIndexEntry {
                local_key: pi as u32,
                ref_count: 1,
                key: &pd.id,
            });
        }
        write_set_index(
            comp,
            &cls_path,
            "Properties-9",
            PID_METADEF_IDENT,
            16,
            &mut prop_entries,
        )?;

        cls_entries.push(SetIndexEntry {
            local_key: ci as u32,
            ref_count: 1,
            key: &mc.id,
        });
    }
    write_set_index(
        comp,
        md,
        "ClassDefinitions-3",
        PID_METADEF_IDENT,
        16,
        &mut cls_entries,
    )?;

    let mut typ_entries: Vec<SetIndexEntry<'_>> = Vec::with_capacity(META_TYPES.len());
    for (ti, mt) in META_TYPES.iter().enumerate() {
        let tp = format!("{md}/TypeDefinitions-4{{{ti:x}}}");
        let mut props: Vec<(u16, u16, Vec<u8>)> = vec![
            (PID_METADEF_IDENT, SF_DATA, mt.id.to_vec()),
            (PID_METADEF_NAME, SF_DATA, encode_utf16le_nul(mt.name)),
        ];
        let mut record_types: Option<&[[u8; 16]]> = None;
        match &mt.kind {
            MetaTypeKind::Integer { size, signed } => {
                props.push((PID_INT_SIZE, SF_DATA, vec![*size]));
                props.push((PID_INT_IS_SIGNED, SF_DATA, vec![u8::from(*signed)]));
            }
            MetaTypeKind::Character => {}
            MetaTypeKind::String { elem } => {
                props.push((
                    PID_STRING_ELEM_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, elem),
                ));
            }
            MetaTypeKind::Rename { target } => {
                props.push((
                    PID_RENAME_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, target),
                ));
            }
            MetaTypeKind::StrongRef { target_cls } => {
                props.push((
                    PID_STRONGREF_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_CLASSES, PID_METADEF_IDENT, target_cls),
                ));
            }
            MetaTypeKind::WeakRef {
                target_cls,
                target_set,
            } => {
                props.push((
                    PID_WEAKREF_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_CLASSES, PID_METADEF_IDENT, target_cls),
                ));
                let mut ts = Vec::with_capacity(target_set.len() * 16);
                for a in *target_set {
                    ts.extend_from_slice(a);
                }
                props.push((PID_WEAKREF_TARGET_SET, SF_DATA, ts));
            }
            MetaTypeKind::Enum {
                elem,
                names,
                values,
            } => {
                props.push((
                    PID_ENUM_ELEMENT_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, elem),
                ));
                props.push((PID_ENUM_ELEMENT_NAMES, SF_DATA, pack_utf16le_strings(names)));
                let mut vs = Vec::with_capacity(values.len() * 8);
                for v in *values {
                    vs.extend_from_slice(&v.to_le_bytes());
                }
                props.push((PID_ENUM_ELEMENT_VALUES, SF_DATA, vs));
            }
            MetaTypeKind::FixedArray { elem, count } => {
                props.push((
                    PID_FIXEDARRAY_ELEM_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, elem),
                ));
                props.push((
                    PID_FIXEDARRAY_ELEM_COUNT,
                    SF_DATA,
                    count.to_le_bytes().to_vec(),
                ));
            }
            MetaTypeKind::VarArray { elem } => {
                props.push((
                    PID_VARARRAY_ELEM_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, elem),
                ));
            }
            MetaTypeKind::Set { elem } => {
                props.push((
                    PID_SET_ELEM_TYPE,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_META_TYPES, PID_METADEF_IDENT, elem),
                ));
            }
            MetaTypeKind::Record { names, types } => {
                props.push((
                    PID_RECORD_MEMBER_NAMES,
                    SF_DATA,
                    pack_utf16le_strings(names),
                ));
                props.push((
                    PID_RECORD_MEMBER_TYPES,
                    SF_WEAK_REF_VECTOR,
                    encode_utf16le_nul("MemberTypes-1c"),
                ));
                record_types = Some(*types);
            }
        }
        create_object_clsid(comp, &tp, Uuid::from_bytes_le(mt.class_id), &props)?;
        if let Some(mtypes) = record_types {
            write_weak_ref_vector_index(
                comp,
                &tp,
                "MemberTypes-1c",
                REF_IDX_META_TYPES,
                PID_METADEF_IDENT,
                mtypes,
            )?;
        }
        typ_entries.push(SetIndexEntry {
            local_key: ti as u32,
            ref_count: 1,
            key: &mt.id,
        });
    }
    write_set_index(
        comp,
        md,
        "TypeDefinitions-4",
        PID_METADEF_IDENT,
        16,
        &mut typ_entries,
    )?;
    Ok(())
}
