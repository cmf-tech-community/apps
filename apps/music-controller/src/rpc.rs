//! The dedicated Music Companion RPC, watch side.
//!
//! Requests are `M1Q id method args`; replies are `M1A id status index total record`.
//! The `M1` namespace is intentionally separate from FMC Companion's generic `R1` namespace, so both phone
//! apps can coexist without answering each other's requests.
//!
//! Only non-personal playback state and music volume cross the BLE channel. Track names, artists,
//! notification text and artwork never do.

pub const MUSIC_SNAPSHOT: u8 = 0x60;
pub const MUSIC_COMMAND: u8 = 0x61;

pub const OK: u8 = 0;
pub const BAD_REQUEST: u8 = 1;
pub const DENIED: u8 = 2;
pub const UNAVAILABLE: u8 = 3;
pub const ASK: u8 = 4;

pub const RESPONSE_PLAYING: u8 = 1;
pub const RESPONSE_ACTIVE: u8 = 2;

pub const PLAY: u8 = 0;
pub const PAUSE: u8 = 1;
pub const TOGGLE: u8 = 2;
pub const PREVIOUS: u8 = 3;
pub const NEXT: u8 = 4;
pub const VOLUME_UP: u8 = 5;
pub const VOLUME_DOWN: u8 = 6;
pub const SET_VOLUME: u8 = 7;

/// A request that fits the app-message channel.
#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum Request {
    Snapshot([u8; 6]),
    Command([u8; 7]),
}

impl Request {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Request::Snapshot(bytes) => bytes,
            Request::Command(bytes) => bytes,
        }
    }
}

/// Requests the current non-personal playback state.
pub fn snapshot_request(id: u8) -> Request {
    Request::Snapshot([b'M', b'1', b'Q', id, MUSIC_SNAPSHOT, 0])
}

/// `music.command`; `value` is used only by `SET_VOLUME`.
pub fn command_request(id: u8, command: u8, value: u8) -> Request {
    Request::Command([b'M', b'1', b'Q', id, MUSIC_COMMAND, command, value])
}

#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub struct Reply<'a> {
    pub id: u8,
    pub status: u8,
    pub index: u8,
    pub total: u8,
    pub record: &'a [u8],
}

/// A reply on this channel; anything else is ignored.
pub fn reply(message: &[u8]) -> Option<Reply<'_>> {
    match message {
        [b'M', b'1', b'A', id, status, index, total, record @ ..]
            if *status <= ASK && record.len() <= 121 =>
        {
            Some(Reply {
                id: *id,
                status: *status,
                index: *index,
                total: *total,
                record,
            })
        }
        _ => None,
    }
}

/// Non-personal playback state from the snapshot record.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub struct Metadata {
    pub flags: u8,
    pub volume: u8,
    pub max_volume: u8,
}

impl Metadata {
    pub const EMPTY: Metadata = Metadata {
        flags: 0,
        volume: 0,
        max_volume: 0,
    };

    pub fn playing(&self) -> bool {
        self.flags & RESPONSE_PLAYING != 0
    }

    pub fn active(&self) -> bool {
        self.flags & RESPONSE_ACTIVE != 0
    }

    /// Whether two records contain exactly the same cache data.
    pub fn same_data(&self, other: &Metadata) -> bool {
        self.flags == other.flags
            && self.volume == other.volume
            && self.max_volume == other.max_volume
    }

    /// Encodes the metadata record for the watch's local cache.
    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < 4 {
            return None;
        }
        out[0] = 0;
        out[1] = self.flags;
        out[2] = self.volume;
        out[3] = self.max_volume;
        Some(4)
    }

    /// Decodes `0:u8 || flags || volume || max_volume`.
    pub fn decode(record: &[u8]) -> Option<Metadata> {
        if record.len() != 4 || record[0] != 0 {
            return None;
        }
        let flags = record[1];
        if flags & !(RESPONSE_PLAYING | RESPONSE_ACTIVE) != 0 || record[2] > record[3] {
            return None;
        }
        Some(Metadata {
            flags,
            volume: record[2],
            max_volume: record[3],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn metadata_vector_decodes() {
        let metadata = Metadata::decode(&hex("0003070f")).unwrap();
        assert_eq!(metadata.flags, 3);
        assert!(metadata.playing() && metadata.active());
        assert_eq!((metadata.volume, metadata.max_volume), (7, 15));
    }

    #[test]
    fn malformed_metadata_is_rejected() {
        // Wrong type, wrong length, unknown flag, and volume above maximum are all rejected.
        assert!(Metadata::decode(&hex("0103070f")).is_none());
        assert!(Metadata::decode(&hex("000307")).is_none());
        assert!(Metadata::decode(&hex("0008070f")).is_none());
        assert!(Metadata::decode(&hex("00030f07")).is_none());
    }

    #[test]
    fn requests_match_the_wire_layout() {
        assert_eq!(snapshot_request(8).bytes(), &hex("4d3151086000"));
        assert_eq!(command_request(10, NEXT, 0).bytes(), &hex("4d31510a610400"));
        assert_eq!(
            command_request(11, SET_VOLUME, 9).bytes(),
            &hex("4d31510b610709")
        );
    }

    #[test]
    fn replies_are_parsed() {
        let first_bytes = hex("4d314107000000");
        let first = reply(&first_bytes).unwrap();
        assert_eq!(
            (
                first.id,
                first.status,
                first.index,
                first.total,
                first.record
            ),
            (7, OK, 0, 0, &[][..])
        );
        let second_bytes = hex("4d3141080000010003070f");
        let second = reply(&second_bytes).unwrap();
        assert_eq!(second.total, 1);
        assert_eq!(second.record, &hex("0003070f")[..]);
        let invalid = hex("4d31510701");
        assert!(reply(&invalid).is_none());
        assert!(reply(&hex("52314105000000")).is_none());
    }
}
