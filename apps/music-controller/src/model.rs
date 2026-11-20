//! The phone-music state machine: requests, optimistic commands and the local cache.
//!
//! One RPC request is in flight at a time. Commands tapped while a snapshot is in flight wait in a
//! small local queue. Only playback flags and music volume cross the BLE channel; no track text or
//! artwork is accepted or retained.

use crate::rpc::{self, Metadata};

pub const POLL_MS: u32 = 2_000;
pub const RETRY_MS: u32 = 3_000;
pub const ASK_AGAIN_MS: u32 = 10_000;
pub const COMMAND_REFRESH_MS: u32 = 250;
pub const SNAPSHOT_TIMEOUT_MS: u32 = 10_000;
pub const COMMAND_TIMEOUT_MS: u32 = 8_000;
pub const MAX_BACKOFF_MS: u32 = 30_000;

const COMMAND_QUEUE_MAX: usize = 8;
const MAGIC: &[u8; 2] = b"M2";
/// Magic, fetched wall minute, then the four-byte metadata record.
pub const BLOB_MAX: usize = 2 + 4 + 4;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Status {
    Waiting,
    Ok,
    Ask,
    Denied,
    Unavailable,
    /// The latest request got no answer.
    Silent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Got {
    Nothing,
    /// The screen changed.
    Changed,
    /// Metadata changed and should be written to the watch's local storage.
    Save,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Active {
    Snapshot {
        id: u8,
        started: u32,
    },
    Command {
        id: u8,
        command: u8,
        value: u8,
        before: Metadata,
        started: u32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
struct Command {
    code: u8,
    value: u8,
    before: Metadata,
}

pub struct Model {
    pub metadata: Metadata,
    pub has_snapshot: bool,
    pub status: Status,
    /// Wall minute of the last committed snapshot.
    pub fetched: Option<i32>,
    /// Bumped on every visible change.
    pub rev: u32,

    id: u8,
    next_poll: u32,
    next_command: u32,
    backoff: u32,
    active: Option<Active>,
    commands: [Command; COMMAND_QUEUE_MAX],
    command_head: usize,
    command_len: usize,
    cache_dirty: bool,
}

impl Model {
    pub const fn new() -> Self {
        Model {
            metadata: Metadata::EMPTY,
            has_snapshot: false,
            status: Status::Waiting,
            fetched: None,
            rev: 0,
            id: 0,
            next_poll: 0,
            next_command: 0,
            backoff: RETRY_MS,
            active: None,
            commands: [Command {
                code: 0,
                value: 0,
                before: Metadata::EMPTY,
            }; COMMAND_QUEUE_MAX],
            command_head: 0,
            command_len: 0,
            cache_dirty: false,
        }
    }

    /// Ask at once (start or RESUME). A reply from the previous run has a different id and is ignored.
    pub fn ask_now(&mut self, now: u32) {
        self.active = None;
        self.backoff = RETRY_MS;
        self.next_poll = now;
        if self.command_len > 0 {
            self.next_command = now;
        }
        self.rev += 1;
    }

    /// Whether the current metadata represents an active media session.
    pub fn active(&self) -> bool {
        self.metadata.active()
    }

    /// The current music stream volume.
    pub fn volume(&self) -> u8 {
        self.metadata.volume
    }

    /// The maximum music stream volume.
    pub fn max_volume(&self) -> u8 {
        self.metadata.max_volume
    }

    /// Whether the current media session is playing.
    pub fn playing(&self) -> bool {
        self.metadata.playing()
    }

    /// The next request to send, if any.
    pub fn poll(&mut self, now: u32) -> Option<rpc::Request> {
        if let Some(active) = self.active {
            if now.wrapping_sub(started_at(active)) >= timeout_for(active) {
                if matches!(active, Active::Command { .. }) {
                    self.send_failed(now);
                } else {
                    self.active = None;
                    self.status = Status::Silent;
                    self.backoff = (self.backoff * 2).min(MAX_BACKOFF_MS);
                    self.next_poll = now.wrapping_add(self.backoff);
                    self.rev += 1;
                }
            }
            return None;
        }

        if self.command_len > 0 && due(now, self.next_command) {
            let command = self.commands[self.command_head];
            self.command_head = (self.command_head + 1) % COMMAND_QUEUE_MAX;
            self.command_len -= 1;
            self.id = self.id.wrapping_add(1);
            self.active = Some(Active::Command {
                id: self.id,
                command: command.code,
                value: command.value,
                before: command.before,
                started: now,
            });
            return Some(rpc::command_request(self.id, command.code, command.value));
        }

        if !due(now, self.next_poll) {
            return None;
        }
        self.id = self.id.wrapping_add(1);
        self.active = Some(Active::Snapshot {
            id: self.id,
            started: now,
        });
        Some(rpc::snapshot_request(self.id))
    }

    /// Queue a playback or volume command and update the visible state immediately.
    pub fn command(&mut self, command: u8, value: u8) -> bool {
        if self.command_len == COMMAND_QUEUE_MAX
            || !matches!(
                command,
                rpc::PLAY
                    | rpc::PAUSE
                    | rpc::TOGGLE
                    | rpc::PREVIOUS
                    | rpc::NEXT
                    | rpc::VOLUME_UP
                    | rpc::VOLUME_DOWN
                    | rpc::SET_VOLUME
            )
        {
            return false;
        }

        let old = self.metadata;
        let before = old;
        apply_command(&mut self.metadata, command, value);

        let at = (self.command_head + self.command_len) % COMMAND_QUEUE_MAX;
        self.commands[at] = Command {
            code: command,
            value,
            before,
        };
        if self.command_len == 0 {
            self.next_command = 0;
        }
        self.command_len += 1;

        if !old.same_data(&self.metadata) {
            self.cache_dirty = true;
            self.rev += 1;
        }
        true
    }

    /// Recover immediately when the loader could not queue a request. A command is put back at the
    /// front of the queue so a transient outbox or host failure does not silently drop it.
    pub fn send_failed(&mut self, now: u32) {
        let Some(active) = self.active.take() else {
            return;
        };
        match active {
            Active::Snapshot { .. } => {
                self.next_poll = now.wrapping_add(self.backoff);
            }
            Active::Command {
                command,
                value,
                before,
                ..
            } => {
                if self.command_len < COMMAND_QUEUE_MAX {
                    self.command_head =
                        (self.command_head + COMMAND_QUEUE_MAX - 1) % COMMAND_QUEUE_MAX;
                    self.commands[self.command_head] = Command {
                        code: command,
                        value,
                        before,
                    };
                    self.command_len += 1;
                }
                self.next_command = now.wrapping_add(self.backoff);
                self.next_poll = now.wrapping_add(self.backoff);
            }
        }
        self.status = Status::Silent;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF_MS);
        self.rev += 1;
    }

    /// Handle one host message.
    pub fn reply(&mut self, message: &[u8], now: u32, wall: i32) -> Got {
        let Some(reply) = rpc::reply(message) else {
            return Got::Nothing;
        };
        let Some(active) = self.active else {
            return Got::Nothing;
        };
        match active {
            Active::Command { id, before, .. } if id == reply.id => {
                self.active = None;
                self.next_poll = now.wrapping_add(COMMAND_REFRESH_MS);
                self.next_command = now.wrapping_add(COMMAND_REFRESH_MS);
                self.status = if reply.status == rpc::OK {
                    Status::Ok
                } else {
                    status_of(reply.status)
                };
                let rolled_back = reply.status != rpc::OK && self.rollback_command(before);
                self.rev += 1;
                if rolled_back {
                    self.cache_dirty = true;
                    Got::Save
                } else {
                    Got::Changed
                }
            }
            Active::Snapshot { id, .. } if id == reply.id => self.snapshot(reply, now, wall),
            _ => Got::Nothing,
        }
    }

    /// Restore the state before a denied command, then reapply commands still queued behind it.
    fn rollback_command(&mut self, before: Metadata) -> bool {
        let old = self.metadata;
        self.metadata = before;
        self.reapply_queued_commands();
        !old.same_data(&self.metadata)
    }

    /// Reapply queued optimistic commands and record each command's new rollback baseline.
    fn reapply_queued_commands(&mut self) {
        let mut state = self.metadata;
        for offset in 0..self.command_len {
            let index = (self.command_head + offset) % COMMAND_QUEUE_MAX;
            let mut command = self.commands[index];
            command.before = state;
            apply_command(&mut state, command.code, command.value);
            self.commands[index] = command;
        }
        self.metadata = state;
    }

    fn snapshot(&mut self, reply: rpc::Reply<'_>, now: u32, wall: i32) -> Got {
        if reply.status != rpc::OK {
            self.active = None;
            self.status = status_of(reply.status);
            self.next_poll = now.wrapping_add(if self.status == Status::Ask {
                ASK_AGAIN_MS
            } else {
                RETRY_MS
            });
            self.rev += 1;
            return Got::Changed;
        }
        if reply.total != 1 || reply.index != 0 {
            return self.reject_snapshot(now);
        }
        let Some(metadata) = Metadata::decode(reply.record) else {
            return self.reject_snapshot(now);
        };
        self.commit(metadata, now, wall)
    }

    fn reject_snapshot(&mut self, now: u32) -> Got {
        self.active = None;
        self.status = Status::Unavailable;
        self.next_poll = now.wrapping_add(RETRY_MS);
        self.rev += 1;
        Got::Changed
    }

    fn commit(&mut self, metadata: Metadata, now: u32, wall: i32) -> Got {
        let old = self.metadata;
        self.metadata = metadata;
        self.reapply_queued_commands();
        self.has_snapshot = true;
        self.fetched = Some(wall);
        self.status = Status::Ok;
        self.backoff = RETRY_MS;
        self.next_poll = now.wrapping_add(POLL_MS);
        self.active = None;

        if !old.same_data(&metadata) {
            self.rev += 1;
            self.cache_dirty = true;
            Got::Save
        } else {
            Got::Changed
        }
    }

    /// The cache blob. It holds non-personal playback state and volume only.
    pub fn save(&mut self, out: &mut [u8]) -> usize {
        if !self.has_snapshot {
            return 0;
        }
        let Some(wall) = self.fetched else { return 0 };
        out[..2].copy_from_slice(MAGIC);
        out[2..6].copy_from_slice(&wall.to_le_bytes());
        let Some(len) = self.metadata.encode(&mut out[6..]) else {
            return 0;
        };
        6 + len
    }

    /// Whether metadata has changed since the last successful storage write.
    pub fn cache_dirty(&self) -> bool {
        self.cache_dirty
    }

    /// Mark the current metadata as durably stored.
    pub fn cache_saved(&mut self) {
        self.cache_dirty = false;
    }

    /// Restore cached non-personal playback state.
    pub fn load(&mut self, blob: &[u8]) {
        if blob.len() < 10 || &blob[..2] != MAGIC {
            return;
        }
        let Some(metadata) = Metadata::decode(&blob[6..10]) else {
            return;
        };
        self.metadata = metadata;
        self.has_snapshot = true;
        self.fetched = Some(i32::from_le_bytes([blob[2], blob[3], blob[4], blob[5]]));
        self.cache_dirty = false;
        self.status = Status::Waiting;
        self.rev += 1;
    }

    /// Ms until the next request or timeout.
    pub fn wait(&self, now: u32) -> u32 {
        let next = if let Some(active) = self.active {
            started_at(active).wrapping_add(timeout_for(active))
        } else if self.command_len > 0 {
            self.next_command.min(self.next_poll)
        } else {
            self.next_poll
        };
        next.wrapping_sub(now).min(SNAPSHOT_TIMEOUT_MS)
    }
}

fn apply_command(metadata: &mut Metadata, command: u8, value: u8) {
    match command {
        rpc::PLAY => {
            metadata.flags |= rpc::RESPONSE_PLAYING | rpc::RESPONSE_ACTIVE;
        }
        rpc::PAUSE => metadata.flags &= !rpc::RESPONSE_PLAYING,
        rpc::TOGGLE => {
            if metadata.active() && metadata.playing() {
                metadata.flags &= !rpc::RESPONSE_PLAYING;
            } else {
                metadata.flags |= rpc::RESPONSE_PLAYING | rpc::RESPONSE_ACTIVE;
            }
        }
        rpc::VOLUME_UP => {
            let max = metadata.max_volume;
            metadata.volume = metadata.volume.saturating_add(1).min(max);
        }
        rpc::VOLUME_DOWN => metadata.volume = metadata.volume.saturating_sub(1),
        rpc::SET_VOLUME => metadata.volume = value.min(metadata.max_volume),
        rpc::PREVIOUS | rpc::NEXT => {}
        _ => {}
    }
}

fn due(now: u32, at: u32) -> bool {
    now.wrapping_sub(at) as i32 >= 0
}

fn started_at(active: Active) -> u32 {
    match active {
        Active::Snapshot { started, .. } | Active::Command { started, .. } => started,
    }
}

fn timeout_for(active: Active) -> u32 {
    match active {
        Active::Snapshot { .. } => SNAPSHOT_TIMEOUT_MS,
        Active::Command { .. } => COMMAND_TIMEOUT_MS,
    }
}

fn status_of(status: u8) -> Status {
    match status {
        rpc::OK => Status::Ok,
        rpc::ASK => Status::Ask,
        rpc::DENIED => Status::Denied,
        rpc::BAD_REQUEST | rpc::UNAVAILABLE => Status::Unavailable,
        _ => Status::Unavailable,
    }
}

/// A compact, fixed-size text buffer used by the view.
pub struct Buf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Buf<N> {
    pub const fn new() -> Self {
        Buf {
            bytes: [0; N],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    fn push_byte(&mut self, byte: u8) {
        if self.len < N {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }

    fn push_u8(&mut self, value: u8) {
        if value >= 100 {
            self.push_byte(b'0' + value / 100);
            self.push_byte(b'0' + (value / 10) % 10);
        } else if value >= 10 {
            self.push_byte(b'0' + value / 10);
        }
        self.push_byte(b'0' + value % 10);
    }
}

/// Writes a volume percentage without allocating or pulling in the formatting machinery.
pub fn percent(out: &mut Buf<8>, volume: u8, max: u8) {
    let value = if max == 0 {
        0
    } else {
        (volume as u32 * 100 / max as u32).min(100)
    } as u8;
    out.push_u8(value);
    out.push_byte(b'%');
}

/// Days since 1970-01-01 of a civil date (H. Hinnant's `days_from_civil`).
pub fn days(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i32 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i32 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Local wall-clock minutes since 1970-01-01.
pub fn wall_min(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i32 {
    days(y, m, d) * 1440 + (hour * 60 + minute) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc;

    fn reply(id: u8, status: u8, index: u8, total: u8, record: &[u8]) -> Vec<u8> {
        let mut out = vec![b'M', b'1', b'A', id, status, index, total];
        out.extend_from_slice(record);
        out
    }

    fn metadata_record(flags: u8, volume: u8, max_volume: u8) -> Vec<u8> {
        vec![0, flags, volume, max_volume]
    }

    #[test]
    fn metadata_snapshot_commits() {
        let mut model = Model::new();
        let request = model.poll(0).unwrap();
        let id = match request {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        let record = metadata_record(3, 7, 15);
        assert_eq!(
            model.reply(&reply(id, 0, 0, 1, &record), 10, 100),
            Got::Save
        );
        assert!(model.has_snapshot);
        assert!(model.active() && model.playing());
        assert_eq!((model.volume(), model.max_volume()), (7, 15));
        assert_eq!(model.status, Status::Ok);
    }

    #[test]
    fn malformed_snapshot_is_rejected_without_committing() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        assert_eq!(
            model.reply(&reply(id, 0, 0, 1, &[0, 3, 16, 15]), 10, 100),
            Got::Changed
        );
        assert!(!model.has_snapshot);
        assert_eq!(model.status, Status::Unavailable);
        assert!(model.poll(10 + RETRY_MS).is_some());
    }

    #[test]
    fn commands_are_queued_behind_a_snapshot() {
        let mut model = Model::new();
        model.poll(0);
        assert!(model.command(rpc::TOGGLE, 0));
        assert!(model.metadata.playing());
        assert!(model.poll(10).is_none());
        assert!(model.poll(SNAPSHOT_TIMEOUT_MS).is_none());
        assert_eq!(model.status, Status::Silent);
        let id = match model.poll(SNAPSHOT_TIMEOUT_MS + 1).unwrap() {
            rpc::Request::Command(bytes) => bytes[3],
            _ => panic!("expected queued command"),
        };
        model.reply(&reply(id, 0, 0, 0, &[]), SNAPSHOT_TIMEOUT_MS + 11, 100);
        assert!(model.poll(SNAPSHOT_TIMEOUT_MS + 12).is_none());
        assert!(matches!(
            model.poll(SNAPSHOT_TIMEOUT_MS + 12 + COMMAND_REFRESH_MS + 1),
            Some(rpc::Request::Snapshot(_))
        ));
    }

    #[test]
    fn optimistic_volume_is_clamped() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100);
        model.command(rpc::SET_VOLUME, 200);
        assert_eq!(model.volume(), 15);
        model.command(rpc::VOLUME_UP, 0);
        assert_eq!(model.volume(), 15);
        model.command(rpc::VOLUME_DOWN, 0);
        assert_eq!(model.volume(), 14);
    }

    #[test]
    fn queue_overflow_does_not_change_visible_state() {
        let mut model = Model::new();
        for _ in 0..COMMAND_QUEUE_MAX {
            assert!(model.command(rpc::VOLUME_UP, 0));
        }
        let volume = model.volume();
        assert!(!model.command(rpc::VOLUME_UP, 0));
        assert_eq!(model.volume(), volume);
    }

    #[test]
    fn unchanged_metadata_does_not_bump_the_visible_revision() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100);
        let rev = model.rev;
        let id = match model.poll(POLL_MS + 10).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(
            &reply(id, 0, 0, 1, &metadata_record(3, 7, 15)),
            POLL_MS + 11,
            100,
        );
        assert_eq!(model.rev, rev);
    }

    #[test]
    fn cache_round_trip_keeps_non_personal_state() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 1234);
        let mut blob = [0u8; BLOB_MAX];
        let len = model.save(&mut blob);
        assert_eq!(len, BLOB_MAX);
        let mut restored = Model::new();
        restored.load(&blob[..len]);
        assert!(restored.has_snapshot);
        assert!(restored.active() && restored.playing());
        assert_eq!(restored.volume(), 7);
        assert_eq!(restored.fetched, Some(1234));
    }

    #[test]
    fn a_failed_command_is_queued_again() {
        let mut model = Model::new();
        assert!(model.command(rpc::TOGGLE, 0));
        let request = model.poll(0).unwrap();
        assert!(matches!(request, rpc::Request::Command(_)));
        model.send_failed(1);
        assert_eq!(model.status, Status::Silent);
        let retry_at = 1 + RETRY_MS;
        let request = model.poll(retry_at).unwrap();
        assert!(matches!(request, rpc::Request::Command(_)));
    }

    #[test]
    fn a_timed_out_command_is_queued_again() {
        let mut model = Model::new();
        assert!(model.command(rpc::TOGGLE, 0));
        assert!(matches!(model.poll(0), Some(rpc::Request::Command(_))));
        assert!(model.poll(COMMAND_TIMEOUT_MS).is_none());
        assert_eq!(model.status, Status::Silent);
        let retry_at = COMMAND_TIMEOUT_MS + RETRY_MS * 2;
        assert!(matches!(
            model.poll(retry_at),
            Some(rpc::Request::Command(_))
        ));
    }

    #[test]
    fn an_optimistic_command_marks_the_cache_dirty() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100);
        model.cache_saved();
        assert!(model.command(rpc::VOLUME_UP, 0));
        assert!(model.cache_dirty());
        let mut blob = [0u8; BLOB_MAX];
        assert!(model.save(&mut blob) > 0);
        model.cache_saved();
        assert!(!model.cache_dirty());
    }

    #[test]
    fn denied_command_rolls_back_and_reapplies_later_optimistic_commands() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100);
        model.cache_saved();
        assert!(model.command(rpc::PAUSE, 0));
        assert!(model.command(rpc::VOLUME_UP, 0));
        assert!(!model.playing());
        assert_eq!(model.volume(), 8);

        let id = match model.poll(11).unwrap() {
            rpc::Request::Command(bytes) => bytes[3],
            _ => panic!("expected command"),
        };
        assert_eq!(
            model.reply(&reply(id, rpc::DENIED, 0, 0, &[]), 12, 100),
            Got::Save
        );
        assert!(model.playing());
        assert_eq!(model.volume(), 8);
        assert!(model.cache_dirty());
        assert_eq!(model.status, Status::Denied);
    }

    #[test]
    fn snapshot_while_a_command_is_queued_refreshes_its_rollback_baseline() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100);

        let id = match model.poll(POLL_MS + 10).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        assert!(model.command(rpc::VOLUME_UP, 0));
        assert_eq!(model.volume(), 8);
        model.reply(
            &reply(id, 0, 0, 1, &metadata_record(3, 6, 15)),
            POLL_MS + 11,
            100,
        );
        assert_eq!(model.volume(), 7);

        let id = match model.poll(POLL_MS + 12).unwrap() {
            rpc::Request::Command(bytes) => bytes[3],
            _ => panic!("expected command"),
        };
        assert_eq!(
            model.reply(&reply(id, rpc::DENIED, 0, 0, &[]), POLL_MS + 13, 100),
            Got::Save
        );
        assert_eq!(model.volume(), 6);
    }

    #[test]
    fn permission_and_unavailable_statuses_are_distinct() {
        let mut model = Model::new();
        let id = match model.poll(0).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, rpc::ASK, 0, 0, &[]), 10, 100);
        assert_eq!(model.status, Status::Ask);
        assert!(model.poll(ASK_AGAIN_MS).is_none());
        let id = match model.poll(ASK_AGAIN_MS + 11).unwrap() {
            rpc::Request::Snapshot(bytes) => bytes[3],
            _ => panic!("expected snapshot"),
        };
        model.reply(&reply(id, rpc::DENIED, 0, 0, &[]), ASK_AGAIN_MS + 12, 100);
        assert_eq!(model.status, Status::Denied);
    }

    #[test]
    fn stale_reply_ids_are_ignored() {
        let mut model = Model::new();
        model.poll(0);
        assert_eq!(
            model.reply(&reply(99, 0, 0, 1, &metadata_record(3, 7, 15)), 10, 100),
            Got::Nothing
        );
        assert!(!model.has_snapshot);
    }

    #[test]
    fn wall_clock_uses_local_calendar_time() {
        assert_eq!(wall_min(1970, 1, 1, 0, 0), 0);
        assert_eq!(wall_min(1970, 1, 2, 0, 0), 1440);
        assert_eq!(
            wall_min(2026, 11, 19, 12, 30),
            days(2026, 11, 19) * 1440 + 750
        );
    }
}
