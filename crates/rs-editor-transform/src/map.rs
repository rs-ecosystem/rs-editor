//! Position mapping through document changes (`map.ts`).

/// Which side a position sticks to when content is inserted exactly at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Assoc {
    /// Stay before inserted content.
    Before,
    /// Move after inserted content.
    After,
}

impl Assoc {
    fn sign(self) -> i8 {
        match self {
            Assoc::Before => -1,
            Assoc::After => 1,
        }
    }
}

const DEL_BEFORE: u8 = 1;
const DEL_AFTER: u8 = 2;
const DEL_ACROSS: u8 = 4;
const DEL_SIDE: u8 = 8;

/// A mapped position plus information about deletions around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapResult {
    pub pos: usize,
    del_info: u8,
    /// Lets a mirrored map recover the exact position (see [`Mapping`]).
    pub recover: Option<Recover>,
}

/// Opaque token identifying a position inside a replaced range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recover {
    index: usize,
    offset: usize,
}

impl MapResult {
    /// The position's side (per `assoc`) was deleted.
    pub fn deleted(&self) -> bool {
        self.del_info & DEL_SIDE != 0
    }

    pub fn deleted_before(&self) -> bool {
        self.del_info & (DEL_BEFORE | DEL_ACROSS) != 0
    }

    pub fn deleted_after(&self) -> bool {
        self.del_info & (DEL_AFTER | DEL_ACROSS) != 0
    }

    /// The deletion covered the position on both sides.
    pub fn deleted_across(&self) -> bool {
        self.del_info & DEL_ACROSS != 0
    }
}

/// Anything positions can be mapped through.
pub trait Mappable {
    fn map(&self, pos: usize, assoc: Assoc) -> usize;
    fn map_result(&self, pos: usize, assoc: Assoc) -> MapResult;
}

/// The position changes made by one step: `(start, old_size, new_size)` triples.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepMap {
    ranges: Vec<usize>,
    inverted: bool,
}

impl StepMap {
    pub fn new(ranges: Vec<usize>) -> Self {
        debug_assert!(ranges.len().is_multiple_of(3));
        Self {
            ranges,
            inverted: false,
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    /// A map that shifts every position by `n`.
    pub fn offset(n: isize) -> Self {
        match n {
            0 => Self::empty(),
            n if n < 0 => Self::new(vec![0, n.unsigned_abs(), 0]),
            n => Self::new(vec![0, 0, n as usize]),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    fn sizes(&self) -> (usize, usize) {
        if self.inverted { (2, 1) } else { (1, 2) }
    }

    pub fn recover(&self, value: Recover) -> usize {
        let mut diff: isize = 0;
        if !self.inverted {
            for i in 0..value.index {
                diff += self.ranges[i * 3 + 2] as isize - self.ranges[i * 3 + 1] as isize;
            }
        }
        (self.ranges[value.index * 3] as isize + diff) as usize + value.offset
    }

    fn map_inner(&self, pos: usize, assoc: Assoc) -> MapResult {
        let (old_index, new_index) = self.sizes();
        let mut diff: isize = 0;
        let pos_i = pos as isize;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as isize - if self.inverted { diff } else { 0 };
            if start > pos_i {
                break;
            }
            let old_size = self.ranges[i + old_index] as isize;
            let new_size = self.ranges[i + new_index] as isize;
            let end = start + old_size;
            if pos_i <= end {
                let side = if old_size == 0 {
                    assoc.sign()
                } else if pos_i == start {
                    -1
                } else if pos_i == end {
                    1
                } else {
                    assoc.sign()
                };
                let result = start + diff + if side < 0 { 0 } else { new_size };
                let boundary = if assoc == Assoc::Before { start } else { end };
                let recover = (pos_i != boundary).then(|| Recover {
                    index: i / 3,
                    offset: (pos_i - start) as usize,
                });
                let mut del = if pos_i == start {
                    DEL_AFTER
                } else if pos_i == end {
                    DEL_BEFORE
                } else {
                    DEL_ACROSS
                };
                let side_deleted = if assoc == Assoc::Before {
                    pos_i != start
                } else {
                    pos_i != end
                };
                if side_deleted {
                    del |= DEL_SIDE;
                }
                return MapResult {
                    pos: result as usize,
                    del_info: del,
                    recover,
                };
            }
            diff += new_size - old_size;
        }
        MapResult {
            pos: (pos_i + diff) as usize,
            del_info: 0,
            recover: None,
        }
    }

    /// Whether `pos` falls in the range identified by `recover`.
    pub fn touches(&self, pos: usize, recover: Recover) -> bool {
        let (old_index, new_index) = self.sizes();
        let mut diff: isize = 0;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as isize - if self.inverted { diff } else { 0 };
            if start > pos as isize {
                break;
            }
            let old_size = self.ranges[i + old_index] as isize;
            if pos as isize <= start + old_size && i == recover.index * 3 {
                return true;
            }
            diff += self.ranges[i + new_index] as isize - old_size;
        }
        false
    }

    /// Calls `f(old_start, old_end, new_start, new_end)` for each changed range.
    pub fn for_each(&self, mut f: impl FnMut(usize, usize, usize, usize)) {
        let (old_index, new_index) = self.sizes();
        let mut diff: isize = 0;
        for i in (0..self.ranges.len()).step_by(3) {
            let start = self.ranges[i] as isize;
            let old_start = start - if self.inverted { diff } else { 0 };
            let new_start = start + if self.inverted { 0 } else { diff };
            let old_size = self.ranges[i + old_index] as isize;
            let new_size = self.ranges[i + new_index] as isize;
            f(
                old_start as usize,
                (old_start + old_size) as usize,
                new_start as usize,
                (new_start + new_size) as usize,
            );
            diff += new_size - old_size;
        }
    }

    pub fn invert(&self) -> StepMap {
        StepMap {
            ranges: self.ranges.clone(),
            inverted: !self.inverted,
        }
    }
}

impl Mappable for StepMap {
    fn map(&self, pos: usize, assoc: Assoc) -> usize {
        self.map_inner(pos, assoc).pos
    }

    fn map_result(&self, pos: usize, assoc: Assoc) -> MapResult {
        self.map_inner(pos, assoc)
    }
}

/// A pipeline of step maps. Mirrored pairs (a map and its inverse) let
/// positions inside a deleted-then-restored range be recovered exactly.
#[derive(Debug, Clone, Default)]
pub struct Mapping {
    maps: Vec<StepMap>,
    mirror: Vec<(usize, usize)>,
    from: usize,
    to: usize,
}

impl Mapping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_maps(maps: Vec<StepMap>) -> Self {
        let to = maps.len();
        Self {
            maps,
            mirror: Vec::new(),
            from: 0,
            to,
        }
    }

    pub fn maps(&self) -> &[StepMap] {
        &self.maps[self.from..self.to]
    }

    /// The part of this mapping from map `from` to `to`.
    pub fn slice(&self, from: usize, to: usize) -> Mapping {
        Mapping {
            maps: self.maps.clone(),
            mirror: self.mirror.clone(),
            from,
            to,
        }
    }

    pub fn slice_from(&self, from: usize) -> Mapping {
        self.slice(from, self.maps.len())
    }

    pub fn append_map(&mut self, map: StepMap, mirrors: Option<usize>) {
        self.maps.push(map);
        self.to = self.maps.len();
        if let Some(mirror) = mirrors {
            self.set_mirror(self.maps.len() - 1, mirror);
        }
    }

    pub fn append_mapping(&mut self, mapping: &Mapping) {
        let start_size = self.maps.len();
        for i in 0..mapping.maps.len() {
            let mirror = mapping
                .get_mirror(i)
                .filter(|m| *m < i)
                .map(|m| start_size + m);
            self.append_map(mapping.maps[i].clone(), mirror);
        }
    }

    pub fn get_mirror(&self, n: usize) -> Option<usize> {
        self.mirror.iter().find_map(|(a, b)| {
            if *a == n {
                Some(*b)
            } else if *b == n {
                Some(*a)
            } else {
                None
            }
        })
    }

    pub fn set_mirror(&mut self, n: usize, m: usize) {
        self.mirror.push((n, m));
    }

    pub fn append_mapping_inverted(&mut self, mapping: &Mapping) {
        let total = self.maps.len() + mapping.maps.len();
        for i in (0..mapping.maps.len()).rev() {
            let mirror = mapping
                .get_mirror(i)
                .filter(|m| *m > i)
                .map(|m| total - m - 1);
            self.append_map(mapping.maps[i].invert(), mirror);
        }
    }

    pub fn invert(&self) -> Mapping {
        let mut inverse = Mapping::new();
        inverse.append_mapping_inverted(self);
        inverse
    }

    fn map_inner(&self, pos: usize, assoc: Assoc) -> MapResult {
        let mut pos = pos;
        let mut del_info = 0;
        let mut i = self.from;
        while i < self.to {
            let result = self.maps[i].map_result(pos, assoc);
            if let Some(recover) = result.recover
                && let Some(corr) = self.get_mirror(i)
                && corr > i
                && corr < self.to
            {
                i = corr;
                pos = self.maps[corr].recover(recover);
                i += 1;
                continue;
            }
            del_info |= result.del_info;
            pos = result.pos;
            i += 1;
        }
        MapResult {
            pos,
            del_info,
            recover: None,
        }
    }
}

impl Mappable for Mapping {
    fn map(&self, pos: usize, assoc: Assoc) -> usize {
        if self.mirror.is_empty() {
            return self.maps[self.from..self.to]
                .iter()
                .fold(pos, |pos, map| map.map(pos, assoc));
        }
        self.map_inner(pos, assoc).pos
    }

    fn map_result(&self, pos: usize, assoc: Assoc) -> MapResult {
        self.map_inner(pos, assoc)
    }
}
