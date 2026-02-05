use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct GameInfo {
    pub private: bool,
    pub name: String,
    pub max_players: i32,
    pub player_count: i32,
    pub skill: i32,
    pub current_map: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_title: Option<String>,
    #[serde(default)]
    pub server_started_at: Option<i64>,
    #[serde(default)]
    pub map_started_at: Option<i64>,
    pub monster_kill_count: i32,
    pub monster_count: i32,
    pub motd: Option<String>,
    pub sv_cheats: bool,
    pub sv_allowchat: bool,
    pub sv_allowvoicechat: bool,
    pub sv_fastmonsters: bool,
    pub sv_monsters: bool,
    pub sv_nomonsters: bool,
    pub sv_itemsrespawn: bool,
    pub sv_itemrespawntime: Option<i32>,
    pub sv_coop_damagefactor: Option<f32>,
    pub sv_nojump: bool,
    pub sv_nocrouch: bool,
    pub sv_nofreelook: bool,
    pub sv_respawnonexit: bool,
    pub sv_timelimit: Option<i32>,
    pub sv_fraglimit: Option<i32>,
    pub sv_scorelimit: Option<i32>,
    pub sv_duellimit: Option<i32>,
    pub sv_roundlimit: Option<i32>,
    pub sv_allowrun: bool,
    pub sv_allowfreelook: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "op", content = "value", rename_all = "lowercase")]
pub enum Settable<T> {
    Set(T),
    Unset,
}

impl<T> Settable<T> {
    pub fn map(self, f: impl FnOnce(T) -> T) -> Self {
        match self {
            Settable::Set(value) => Settable::Set(f(value)),
            Settable::Unset => Settable::Unset,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ZandronumGameInfoUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_title: Option<Settable<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_players: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_count: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_map: Option<Settable<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_started_at: Option<Settable<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_started_at: Option<Settable<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monster_kill_count: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monster_count: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motd: Option<Settable<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_cheats: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_allowchat: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_allowvoicechat: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_fastmonsters: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_monsters: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_nomonsters: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_itemsrespawn: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_itemrespawntime: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_coop_damagefactor: Option<Settable<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_nojump: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_nocrouch: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_nofreelook: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_respawnonexit: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_timelimit: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_fraglimit: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_scorelimit: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_duellimit: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_roundlimit: Option<Settable<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_allowrun: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sv_allowfreelook: Option<Settable<bool>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GameInfoUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<Settable<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Settable<String>>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub zandronum: Option<Box<ZandronumGameInfoUpdate>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Party {
    pub id: Uuid,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    pub leader_id: Uuid,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<Uuid>>,
}

pub mod wad {
    use serde::{Deserialize, Serialize};
    use std::collections::{BTreeMap, HashMap};
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ReadWad {
        pub meta: ReadWadMeta,
        pub maps: Vec<MapStat>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct InsertWad {
        pub meta: InsertWadMeta,
        pub maps: Vec<MapStat>,
    }

    impl From<InsertWad> for ReadWad {
        fn from(insert: InsertWad) -> Self {
            ReadWad {
                meta: insert.meta.into(),
                maps: insert.maps,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ReadWadMeta {
        #[serde(default, skip_serializing_if = "Uuid::is_nil")]
        pub id: Uuid,

        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub sha1: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sha256: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub authors: Option<Vec<String>>,

        /// All known filenames ever observed for this WAD (from filenames.json).
        /// May include duplicates (e.g., casing differences).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub filenames: Option<Vec<String>>,

        /// A preferred/canonical filename to use (from additional.json:filename), if provided.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub filename: Option<String>,

        /// Additional WAD Archive overrides (additional.json). Stored for provenance.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub additional: Option<AdditionalMeta>,

        /// WAD Archive flags (additional.json).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub flags: Option<WadFlags>,

        /// When this WAD was first indexed in Wad Archive (additional.json:added).
        /// Stored as the original timestamp string.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub added: Option<String>,

        pub file: FileMeta,

        pub content: ContentMeta,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct InsertWadMeta {
        #[serde(default)]
        pub id: Uuid,

        pub sha1: String,
        #[serde(default)]
        pub sha256: Option<String>,

        #[serde(default)]
        pub title: Option<String>,
        #[serde(default)]
        pub authors: Option<Vec<String>>,
        #[serde(default)]
        pub descriptions: Option<Vec<String>>,

        /// All known filenames ever observed for this WAD (from filenames.json).
        /// May include duplicates (e.g., casing differences).
        #[serde(default)]
        pub filenames: Option<Vec<String>>,

        /// A preferred/canonical filename to use (from additional.json:filename), if provided.
        #[serde(default)]
        pub filename: Option<String>,

        /// Additional WAD Archive overrides (additional.json). Stored for provenance.
        #[serde(default)]
        pub additional: Option<AdditionalMeta>,

        /// WAD Archive flags (additional.json).
        #[serde(default)]
        pub flags: Option<WadFlags>,

        /// When this WAD was first indexed in Wad Archive (additional.json:added).
        /// Stored as the original timestamp string.
        #[serde(default)]
        pub added: Option<String>,

        /// Combined extracted PK3 text files + idgames textfile, if any.
        #[serde(default)]
        pub text_files: Option<Vec<TextFile>>,

        pub file: FileMeta,
        pub content: ContentMeta,
        pub sources: SourcesMeta,
    }

    impl From<InsertWadMeta> for ReadWadMeta {
        fn from(insert: InsertWadMeta) -> Self {
            ReadWadMeta {
                id: insert.id,
                sha1: insert.sha1,
                sha256: insert.sha256,
                title: insert.title,
                filenames: insert.filenames,
                filename: insert.filename,
                additional: insert.additional,
                flags: insert.flags,
                authors: insert.authors,
                added: insert.added,
                file: insert.file,
                content: insert.content,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct WadFlags {
        #[serde(default)]
        pub locked: Option<bool>,

        #[serde(default, rename = "canDownload")]
        pub can_download: Option<bool>,

        #[serde(default)]
        pub adult: Option<bool>,

        #[serde(default)]
        pub hidden: Option<bool>,
    }

    /// Partial schema for additional.json. We keep the raw override data for provenance,
    /// but only a subset is interpreted by wadinfo today.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AdditionalMeta {
        #[serde(default)]
        pub engines: Vec<String>,

        /// Note: this key is named `iwad` in additional.json.
        #[serde(default)]
        pub iwad: Vec<String>,

        #[serde(default)]
        pub filename: Option<String>,

        #[serde(default)]
        pub added: Option<String>,

        #[serde(default)]
        pub locked: bool,

        #[serde(default, rename = "canDownload")]
        pub can_download: bool,

        #[serde(default)]
        pub adult: bool,

        #[serde(default)]
        pub hidden: bool,

        #[serde(default)]
        pub name: Option<String>,

        #[serde(default)]
        pub description: Option<String>,

        #[serde(default)]
        pub maps: Option<serde_json::Value>,

        #[serde(default, rename = "graphicOverrides")]
        pub graphic_overrides: Option<serde_json::Value>,

        #[serde(default)]
        pub screenshots: Option<serde_json::Value>,

        #[serde(default)]
        pub palettes: Option<Vec<String>>,

        #[serde(default)]
        pub categories: Option<Vec<String>>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TextFile {
        /// "pk3" or "idgames"
        pub source: String,
        #[serde(default)]
        pub name: Option<String>,
        pub contents: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileMeta {
        #[serde(rename = "type")]
        pub file_type: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub size: Option<i64>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub url: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub corrupt: Option<bool>,

        #[serde(
            default,
            rename = "corruptMessage",
            skip_serializing_if = "Option::is_none"
        )]
        pub corrupt_message: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ContentMeta {
        /// Prefer extracted maps if present; else WAD Archive maps.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub maps: Option<Vec<String>>,

        /// From wads.json; dynamic set of counters.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub counts: Option<BTreeMap<String, i64>>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub engines_guess: Option<Vec<String>>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub iwads_guess: Option<Vec<String>>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SourcesMeta {
        pub wad_archive: WadArchiveSource,

        #[serde(default)]
        pub idgames: Option<IdgamesSource>,

        pub extracted: ExtractedSource,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct WadArchiveSource {
        #[serde(default)]
        pub updated: Option<String>,

        /// hashes object from wads.json (md5/sha1/sha256 strings typically)
        #[serde(default)]
        pub hashes: Option<Hashes>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Hashes {
        #[serde(default)]
        pub md5: Option<String>,
        #[serde(default)]
        pub sha1: Option<String>,
        #[serde(default)]
        pub sha256: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct IdgamesSource {
        #[serde(default)]
        pub id: Option<i64>,
        #[serde(default)]
        pub url: Option<String>,
        #[serde(default)]
        pub dir: Option<String>,
        #[serde(default)]
        pub filename: Option<String>,
        #[serde(default)]
        pub date: Option<String>,
        #[serde(default)]
        pub title: Option<String>,
        #[serde(default)]
        pub author: Option<String>,
        #[serde(default)]
        pub credits: Option<String>,
        #[serde(default)]
        pub textfile: Option<String>,
        #[serde(default)]
        pub rating: Option<f64>,
        #[serde(default)]
        pub votes: Option<i64>,
    }

    //
    // ExtractedSource: returned by extract_metadata_from_file(...)
    // - WAD: extract_from_wad_bytes
    // - ZIP/PK3: extract_from_zip_bytes (but compacted: text_files only contain {path,size})
    // - Unknown: several possible shapes
    //

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "format", rename_all = "lowercase")]
    pub enum ExtractedSource {
        /// extract_from_wad_bytes
        Wad {
            #[serde(default)]
            lump_count: Option<i64>,
            #[serde(default)]
            maps: Option<Vec<String>>,
            #[serde(default)]
            text_lumps: Option<Vec<String>>,
            #[serde(default)]
            names: Option<Vec<String>>,
            #[serde(default)]
            authors: Option<Vec<String>>,
            #[serde(default)]
            descriptions: Option<Vec<String>>,
            // note: text_payloads are commented out in script, so omitted
        },

        /// extract_from_zip_bytes (but compacted in sources.extracted: text_files are {path,size} only)
        Zip {
            #[serde(default)]
            embedded_wads: Option<Vec<EmbeddedWadMeta>>,
            #[serde(default)]
            text_files: Option<Vec<ZipTextFileCompact>>,
            #[serde(default)]
            names: Option<Vec<String>>,
            #[serde(default)]
            authors: Option<Vec<String>>,
            #[serde(default)]
            descriptions: Option<Vec<String>>,

            /// present on zip errors
            #[serde(default)]
            error: Option<String>,
        },

        /// Script may emit unknown for many reasons (not wad header, bad zip, s3 resolution failure, etc).
        Unknown {
            #[serde(default)]
            error: Option<String>,
            #[serde(default)]
            note: Option<String>,
            #[serde(default)]
            size: Option<i64>,

            // only present for "Could not resolve S3 object URL"
            #[serde(default)]
            tried_prefixes: Option<Vec<String>>,
            #[serde(default)]
            expected_ext: Option<String>,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EmbeddedWadMeta {
        /// added in extract_from_zip_bytes: wad_meta["path"] = fname
        #[serde(default)]
        pub path: Option<String>,

        /// inner wad format (should be "wad" for these entries, but keep flexible)
        #[serde(default)]
        pub format: Option<String>,

        #[serde(default)]
        pub lump_count: Option<i64>,
        #[serde(default)]
        pub maps: Option<Vec<String>>,
        #[serde(default)]
        pub text_lumps: Option<Vec<String>>,
        #[serde(default)]
        pub names: Option<Vec<String>>,
        #[serde(default)]
        pub authors: Option<Vec<String>>,
        #[serde(default)]
        pub descriptions: Option<Vec<String>>,

        #[serde(default)]
        pub error: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ZipTextFileCompact {
        #[serde(default)]
        pub path: Option<String>,
        #[serde(default)]
        pub size: Option<i64>,
    }

    //
    // Per-map stats: output of map_summary_from_wad_bytes()
    //

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MapStat {
        pub map: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub title: String,
        pub format: String, // "doom" | "hexen" | "unknown"
        pub stats: MapStats,
        pub monsters: MonstersSummary,
        pub items: ItemsSummary,
        pub mechanics: Mechanics,
        pub difficulty: Difficulty,
        pub compatibility: String, // "vanilla_or_boom" | "hexen" | "unknown"
        pub metadata: MapMetadata,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MapStats {
        pub things: i64,
        pub linedefs: i64,
        pub sidedefs: i64,
        pub vertices: i64,
        pub sectors: i64,
        pub segs: i64,
        pub ssectors: i64,
        pub nodes: i64,

        /// Texture usage histogram: texture name -> count.
        /// Always emitted as an object ({} when empty).
        #[serde(default)]
        pub textures: HashMap<String, i32>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MonstersSummary {
        pub total: i64,

        /// Monster id -> count (sorted in the producer, but JSON object order isn’t guaranteed)
        #[serde(default)]
        pub by_type: BTreeMap<String, i64>,

        /// Optional derived breakdown for vanilla Doom II types.
        /// Keys: melee | hitscanner | projectile | boss
        #[serde(default)]
        pub by_category: Option<BTreeMap<String, i64>>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ItemsSummary {
        pub total: i64,
        #[serde(default)]
        pub by_type: BTreeMap<String, i64>,

        /// Optional derived ammo totals by category.
        /// Keys: bullets | shells | rockets | cells
        #[serde(default)]
        pub ammo_by_category: Option<BTreeMap<String, i64>>,

        /// Optional derived set of weapon ids present on the map.
        #[serde(default)]
        pub weapons_present: Option<Vec<String>>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Mechanics {
        pub teleports: bool,

        /// e.g. ["blue", "red", "yellow_skull"]
        #[serde(default)]
        pub keys: Vec<String>,

        pub secret_exit: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Difficulty {
        pub uv_monsters: i64,
        pub hmp_monsters: i64,
        pub htr_monsters: i64,

        pub uv_items: i64,
        pub hmp_items: i64,
        pub htr_items: i64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MapMetadata {
        /// always null in the current script
        #[serde(default)]
        pub title: Option<String>,
        /// always null in the current script
        #[serde(default)]
        pub music: Option<String>,
        /// always "marker" in the current script
        pub source: String,
    }
}
