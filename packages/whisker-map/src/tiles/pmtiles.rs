//! Tiles read by byte range from a PMTiles v3 archive, over HTTP or from a local file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::geo::TileId;
use crate::net::{FetchError, fetch, gunzip};

const HEADER_LENGTH: u64 = 127;
/// How many directories a lookup may pass through, root included.
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Header {
    root_offset: u64,
    root_length: u64,
    leaves_offset: u64,
    data_offset: u64,
    internal_compression: u8,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// West, south, east, north in degrees.
    pub bounds: [f64; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    tile_id: u64,
    offset: u64,
    length: u64,
    run_length: u64,
}

pub(crate) struct Archive {
    url: String,
    pub header: Header,
    root: Rc<Vec<Entry>>,
    leaves: RefCell<HashMap<u64, Rc<Vec<Entry>>>>,
}

fn error(url: &str, message: &str) -> FetchError {
    FetchError(format!("{url}: {message}"))
}

impl Archive {
    /// Reads the header and the root directory.
    pub async fn open(url: String) -> Result<Archive, FetchError> {
        let bytes = fetch(url.clone(), Some((0, 16384)))
            .await?
            .ok_or_else(|| error(&url, "not found"))?;
        let header =
            Header::parse(&bytes).ok_or_else(|| error(&url, "not a PMTiles v3 archive"))?;
        let start = header.root_offset as usize;
        let end = start + header.root_length as usize;
        let root = match bytes.get(start..end) {
            Some(root) => root.to_vec(),
            None => fetch(url.clone(), Some((header.root_offset, header.root_length)))
                .await?
                .ok_or_else(|| error(&url, "root directory missing"))?,
        };
        let root = decode_directory(&decompress(&root, header.internal_compression)?)
            .ok_or_else(|| error(&url, "bad root directory"))?;
        Ok(Archive {
            url,
            header,
            root: Rc::new(root),
            leaves: RefCell::new(HashMap::new()),
        })
    }

    /// The bytes of tile `id`, still compressed as the archive stores them; `None` when absent.
    pub async fn tile(self: Rc<Self>, id: TileId) -> Result<Option<Vec<u8>>, FetchError> {
        let target = tile_id(id);
        let mut directory = self.root.clone();
        for _ in 0..MAX_DEPTH {
            let Some(entry) = find(&directory, target) else {
                return Ok(None);
            };
            if entry.run_length > 0 {
                return fetch(
                    self.url.clone(),
                    Some((self.header.data_offset + entry.offset, entry.length)),
                )
                .await;
            }
            directory = self.leaf(entry).await?;
        }
        Ok(None)
    }

    async fn leaf(&self, entry: Entry) -> Result<Rc<Vec<Entry>>, FetchError> {
        let offset = self.header.leaves_offset + entry.offset;
        if let Some(leaf) = self.leaves.borrow().get(&offset) {
            return Ok(leaf.clone());
        }
        let bytes = fetch(self.url.clone(), Some((offset, entry.length)))
            .await?
            .ok_or_else(|| error(&self.url, "leaf directory missing"))?;
        let leaf = decode_directory(&decompress(&bytes, self.header.internal_compression)?)
            .ok_or_else(|| error(&self.url, "bad leaf directory"))?;
        let leaf = Rc::new(leaf);
        self.leaves.borrow_mut().insert(offset, leaf.clone());
        Ok(leaf)
    }
}

impl Header {
    fn parse(bytes: &[u8]) -> Option<Header> {
        if bytes.len() < HEADER_LENGTH as usize || &bytes[..7] != b"PMTiles" || bytes[7] != 3 {
            return None;
        }
        let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let degrees =
            |at: usize| f64::from(i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())) / 1e7;
        Some(Header {
            root_offset: u64_at(8),
            root_length: u64_at(16),
            leaves_offset: u64_at(40),
            data_offset: u64_at(56),
            internal_compression: bytes[97],
            min_zoom: bytes[100],
            max_zoom: bytes[101],
            bounds: [degrees(102), degrees(106), degrees(110), degrees(114)],
        })
    }
}

fn decompress(bytes: &[u8], compression: u8) -> Result<Vec<u8>, FetchError> {
    match compression {
        0 | 1 => Ok(bytes.to_vec()),
        2 => gunzip(bytes).map_err(|e| FetchError(e.to_string())),
        other => Err(FetchError(format!(
            "PMTiles compression {other} is not supported"
        ))),
    }
}

fn varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn decode_directory(bytes: &[u8]) -> Option<Vec<Entry>> {
    let mut at = 0;
    let count = varint(bytes, &mut at)? as usize;
    let mut entries = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0,
        };
        count.min(bytes.len())
    ];
    if entries.len() != count {
        return None;
    }
    let mut last = 0u64;
    for entry in &mut entries {
        last += varint(bytes, &mut at)?;
        entry.tile_id = last;
    }
    for entry in &mut entries {
        entry.run_length = varint(bytes, &mut at)?;
    }
    for entry in &mut entries {
        entry.length = varint(bytes, &mut at)?;
    }
    for i in 0..count {
        let value = varint(bytes, &mut at)?;
        entries[i].offset = if value == 0 && i > 0 {
            entries[i - 1].offset + entries[i - 1].length
        } else {
            value.checked_sub(1)?
        };
    }
    Some(entries)
}

/// The entry covering `target`: a run of tiles that contains it, or the leaf directory that may.
fn find(entries: &[Entry], target: u64) -> Option<Entry> {
    let index = entries
        .partition_point(|e| e.tile_id <= target)
        .checked_sub(1)?;
    let entry = entries[index];
    (entry.run_length == 0 || target - entry.tile_id < entry.run_length).then_some(entry)
}

/// The tile's position along the archive's order: by zoom, then along a Hilbert curve.
fn tile_id(id: TileId) -> u64 {
    let z = u32::from(id.z);
    let before: u64 = ((1u64 << (2 * z)) - 1) / 3;
    let n = 1u64 << z;
    let (mut x, mut y) = (u64::from(id.x), u64::from(id.y));
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    before + d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn local_archives_read_by_range() {
        let mut root = Vec::new();
        put_varint(&mut root, 2);
        for value in [0, 2, 1, 1, 3, 4, 1, 0] {
            put_varint(&mut root, value);
        }
        let data_offset = HEADER_LENGTH + root.len() as u64;
        let mut file = vec![0u8; HEADER_LENGTH as usize];
        file[..7].copy_from_slice(b"PMTiles");
        file[7] = 3;
        file[8..16].copy_from_slice(&HEADER_LENGTH.to_le_bytes());
        file[16..24].copy_from_slice(&(root.len() as u64).to_le_bytes());
        file[40..48].copy_from_slice(&data_offset.to_le_bytes());
        file[56..64].copy_from_slice(&data_offset.to_le_bytes());
        file[97] = 1;
        file[101] = 1;
        file.extend_from_slice(&root);
        file.extend_from_slice(b"onetwo!");
        let path = std::env::temp_dir().join(format!("whisker-map-{}.pmtiles", std::process::id()));
        std::fs::write(&path, &file).unwrap();

        let archive = pollster::block_on(Archive::open(format!("file://{}", path.display())));
        let archive = Rc::new(archive.unwrap());
        assert_eq!(archive.header.max_zoom, 1);
        let tile = |id| pollster::block_on(archive.clone().tile(id)).unwrap();
        assert_eq!(tile(TileId::new(0, 0, 0)).as_deref(), Some(&b"one"[..]));
        assert_eq!(tile(TileId::new(1, 0, 1)).as_deref(), Some(&b"two!"[..]));
        assert_eq!(tile(TileId::new(1, 1, 1)), None);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn leaf_directories_are_followed() {
        let mut leaf = Vec::new();
        put_varint(&mut leaf, 2);
        for value in [0, 2, 1, 1, 3, 4, 1, 0] {
            put_varint(&mut leaf, value);
        }
        let mut root = Vec::new();
        put_varint(&mut root, 1);
        for value in [0, 0, leaf.len() as u64, 1] {
            put_varint(&mut root, value);
        }
        let leaves_offset = HEADER_LENGTH + root.len() as u64;
        let data_offset = leaves_offset + leaf.len() as u64;
        let mut file = vec![0u8; HEADER_LENGTH as usize];
        file[..7].copy_from_slice(b"PMTiles");
        file[7] = 3;
        file[8..16].copy_from_slice(&HEADER_LENGTH.to_le_bytes());
        file[16..24].copy_from_slice(&(root.len() as u64).to_le_bytes());
        file[40..48].copy_from_slice(&leaves_offset.to_le_bytes());
        file[56..64].copy_from_slice(&data_offset.to_le_bytes());
        file[97] = 1;
        file[101] = 1;
        file.extend_from_slice(&root);
        file.extend_from_slice(&leaf);
        file.extend_from_slice(b"onetwo!");
        let path =
            std::env::temp_dir().join(format!("whisker-map-leaf-{}.pmtiles", std::process::id()));
        std::fs::write(&path, &file).unwrap();

        let archive = pollster::block_on(Archive::open(format!("file://{}", path.display())));
        let archive = Rc::new(archive.unwrap());
        let tile = |id| pollster::block_on(archive.clone().tile(id)).unwrap();
        assert_eq!(tile(TileId::new(1, 0, 1)).as_deref(), Some(&b"two!"[..]));
        assert_eq!(tile(TileId::new(0, 0, 0)).as_deref(), Some(&b"one"[..]));
        assert_eq!(tile(TileId::new(1, 1, 0)), None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tile_ids_follow_the_hilbert_curve() {
        assert_eq!(tile_id(TileId::new(0, 0, 0)), 0);
        assert_eq!(tile_id(TileId::new(1, 0, 0)), 1);
        assert_eq!(tile_id(TileId::new(1, 0, 1)), 2);
        assert_eq!(tile_id(TileId::new(1, 1, 1)), 3);
        assert_eq!(tile_id(TileId::new(1, 1, 0)), 4);
        assert_eq!(tile_id(TileId::new(2, 0, 0)), 5);
    }

    #[test]
    fn directories_decode_and_find_runs_and_leaves() {
        let mut bytes = Vec::new();
        put_varint(&mut bytes, 3);
        for delta in [0, 5, 10] {
            put_varint(&mut bytes, delta);
        }
        for run in [1, 3, 0] {
            put_varint(&mut bytes, run);
        }
        for length in [10, 20, 40] {
            put_varint(&mut bytes, length);
        }
        for offset in [1, 0, 101] {
            put_varint(&mut bytes, offset);
        }
        let entries = decode_directory(&bytes).unwrap();
        assert_eq!(entries[1].offset, 10);
        assert_eq!(entries[2].offset, 100);
        assert_eq!(find(&entries, 0).unwrap().length, 10);
        assert_eq!(find(&entries, 7).unwrap().length, 20);
        assert_eq!(find(&entries, 8), None);
        assert_eq!(find(&entries, 30).unwrap().run_length, 0);
    }
}
