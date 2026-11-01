//! The compact form a [`SavedScript`] is kept in: a four-byte mark, a
//! version, then every field in a fixed order — integers little-endian,
//! floats as their bits (a NaN or an infinity a script holds comes back as
//! it was), text as a length and UTF-8, a list as a count and its items.

use core::time::Duration;

use sl_types::lsl::{Rotation, Vector};

use crate::bytecode::{FunctionId, StateId};
use crate::operand::Operand;
use crate::value::{Element, Value};
use crate::vm::comms::{Listen, ListenFilter, Listens};
use crate::vm::detected::{Detected, Touch};
use crate::vm::experiences::ExperienceQuestion;
use crate::vm::flavour::EngineFlavour;
use crate::vm::permissions::{Permissions, Question};

use super::{
    SavedActivity, SavedBody, SavedEvent, SavedFrame, SavedMemory, SavedPhase, SavedScript,
    SavedTimer,
};

/// What every saved script starts with.
const MARK: [u8; 4] = *b"slSS";

/// The version of the form this crate writes, and the only one it reads: 2
/// since a listen carries `osListenRegex`'s flags.
const VERSION: u8 = 2;

/// Why bytes are not a saved script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// They do not start with the mark this crate writes.
    #[error("not a saved script")]
    Mark,
    /// A version of the form this crate does not read.
    #[error("saved script version {0} is not one this build reads")]
    Version(u8),
    /// They end in the middle of a field.
    #[error("the saved script is cut short")]
    Truncated,
    /// A tag no field has.
    #[error("{what} tag {tag} is not one this form writes")]
    Tag {
        /// The field.
        what: &'static str,
        /// The tag read.
        tag: u8,
    },
    /// Text that is not UTF-8.
    #[error("saved text is not UTF-8")]
    Text,
    /// Bytes after the end of the script.
    #[error("{0} bytes after the end of the saved script")]
    Trailing(usize),
}

/// Write `saved`.
pub(super) fn encode(saved: &SavedScript) -> Vec<u8> {
    let mut out = Writer(MARK.to_vec());
    out.u8(VERSION);
    out.u8(match saved.flavour {
        EngineFlavour::SecondLife => 0,
        EngineFlavour::YEngine => 1,
        EngineFlavour::XEngine => 2,
    });
    out.values(&saved.globals);
    out.u32(saved.state.0);
    out.bool(saved.running);
    out.bool(saved.held);
    out.count(saved.queue.len());
    for event in &saved.queue {
        out.str(&event.event);
        out.values(&event.args);
        match &event.detected {
            Some(detected) => {
                out.bool(true);
                out.detections(detected);
            }
            None => out.bool(false),
        }
    }
    match &saved.activity {
        Some(activity) => {
            out.bool(true);
            out.activity(activity);
        }
        None => out.bool(false),
    }
    out.state(saved.pending_state);
    out.duration(saved.asleep);
    match saved.timer {
        Some(timer) => {
            out.bool(true);
            out.duration(timer.interval);
            out.duration(timer.next);
        }
        None => out.bool(false),
    }
    out.i32(saved.start_parameter);
    out.detections(&saved.detected);
    out.listens(&saved.listens);
    out.permissions(&saved.permissions);
    out.u32(saved.memory.limit);
    out.u32(saved.memory.peak);
    out.bool(saved.memory.profiling);
    match saved.memory.profiled {
        Some(profiled) => {
            out.bool(true);
            out.u32(profiled);
        }
        None => out.bool(false),
    }
    out.0
}

/// Read a saved script.
pub(super) fn decode(bytes: &[u8]) -> Result<SavedScript, DecodeError> {
    let mut input = Reader { bytes, at: 0 };
    if input.take(MARK.len())? != MARK {
        return Err(DecodeError::Mark);
    }
    let version = input.u8()?;
    if version != VERSION {
        return Err(DecodeError::Version(version));
    }
    let flavour = match input.u8()? {
        0 => EngineFlavour::SecondLife,
        1 => EngineFlavour::YEngine,
        2 => EngineFlavour::XEngine,
        tag => {
            return Err(DecodeError::Tag {
                what: "engine",
                tag,
            });
        }
    };
    let globals = input.values()?;
    let state = StateId(input.u32()?);
    let running = input.bool()?;
    let held = input.bool()?;
    let mut queue = Vec::new();
    for _ in 0..input.count()? {
        let event = input.str()?;
        let args = input.values()?;
        let detected = if input.bool()? {
            Some(input.detections()?)
        } else {
            None
        };
        queue.push(SavedEvent {
            event,
            args,
            detected,
        });
    }
    let activity = if input.bool()? {
        Some(input.activity()?)
    } else {
        None
    };
    let pending_state = input.state()?;
    let asleep = input.duration()?;
    let timer = if input.bool()? {
        Some(SavedTimer {
            interval: input.duration()?,
            next: input.duration()?,
        })
    } else {
        None
    };
    let start_parameter = input.i32()?;
    let detected = input.detections()?;
    let listens = input.listens()?;
    let permissions = input.permissions()?;
    let memory = SavedMemory {
        limit: input.u32()?,
        peak: input.u32()?,
        profiling: input.bool()?,
        profiled: if input.bool()? {
            Some(input.u32()?)
        } else {
            None
        },
    };
    let left = bytes.len().saturating_sub(input.at);
    if left > 0 {
        return Err(DecodeError::Trailing(left));
    }
    Ok(SavedScript {
        flavour,
        globals,
        state,
        running,
        held,
        queue,
        activity,
        pending_state,
        asleep,
        timer,
        start_parameter,
        detected,
        listens,
        permissions,
        memory,
    })
}

/// The form's byte order, little-endian whatever the machine's, so a saved
/// script reads back on any host.
mod le {
    #![expect(
        clippy::little_endian_bytes,
        reason = "the saved form is defined little-endian, whatever the host's order"
    )]

    /// An integer's bytes, as the form writes them.
    pub(super) trait ToLe {
        /// The bytes.
        type Bytes: AsRef<[u8]>;
        /// Its bytes.
        fn to_le(self) -> Self::Bytes;
    }

    impl ToLe for u32 {
        type Bytes = [u8; 4];
        fn to_le(self) -> [u8; 4] {
            self.to_le_bytes()
        }
    }

    impl ToLe for i32 {
        type Bytes = [u8; 4];
        fn to_le(self) -> [u8; 4] {
            self.to_le_bytes()
        }
    }

    impl ToLe for u64 {
        type Bytes = [u8; 8];
        fn to_le(self) -> [u8; 8] {
            self.to_le_bytes()
        }
    }

    /// `number`'s bytes, as the form writes them.
    pub(super) fn to<T: ToLe>(number: T) -> T::Bytes {
        number.to_le()
    }

    /// A `u32` from the form's bytes.
    pub(super) const fn u32_from(bytes: [u8; 4]) -> u32 {
        u32::from_le_bytes(bytes)
    }

    /// An `i32` from the form's bytes.
    pub(super) const fn i32_from(bytes: [u8; 4]) -> i32 {
        i32::from_le_bytes(bytes)
    }

    /// A `u64` from the form's bytes.
    pub(super) const fn u64_from(bytes: [u8; 8]) -> u64 {
        u64::from_le_bytes(bytes)
    }
}

/// The bytes being written.
struct Writer(Vec<u8>);

impl Writer {
    /// One byte.
    fn u8(&mut self, byte: u8) {
        self.0.push(byte);
    }

    /// A flag, as a byte.
    fn bool(&mut self, flag: bool) {
        self.u8(u8::from(flag));
    }

    /// An unsigned 32-bit integer.
    fn u32(&mut self, number: u32) {
        self.0.extend_from_slice(le::to(number).as_ref());
    }

    /// A signed 32-bit integer.
    fn i32(&mut self, number: i32) {
        self.0.extend_from_slice(le::to(number).as_ref());
    }

    /// An unsigned 64-bit integer.
    fn u64(&mut self, number: u64) {
        self.0.extend_from_slice(le::to(number).as_ref());
    }

    /// A count of what follows. Nothing a script holds comes near four
    /// billion of anything; a count past it is written as the most there
    /// is, and the reader runs out of bytes.
    fn count(&mut self, count: usize) {
        self.u32(u32::try_from(count).unwrap_or(u32::MAX));
    }

    /// A float, by its bits.
    fn f32(&mut self, float: f32) {
        self.u32(float.to_bits());
    }

    /// A length of time, in nanoseconds.
    fn duration(&mut self, time: Duration) {
        self.u64(u64::try_from(time.as_nanos()).unwrap_or(u64::MAX));
    }

    /// Text.
    fn str(&mut self, text: &str) {
        self.count(text.len());
        self.0.extend_from_slice(text.as_bytes());
    }

    /// Text that may be absent.
    fn opt_str(&mut self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.bool(true);
                self.str(text);
            }
            None => self.bool(false),
        }
    }

    /// A state that may be absent.
    fn state(&mut self, state: Option<StateId>) {
        match state {
            Some(state) => {
                self.bool(true);
                self.u32(state.0);
            }
            None => self.bool(false),
        }
    }

    /// A vector.
    fn vector(&mut self, vector: &Vector) {
        self.f32(vector.x);
        self.f32(vector.y);
        self.f32(vector.z);
    }

    /// A rotation.
    fn rotation(&mut self, rotation: &Rotation) {
        self.f32(rotation.x);
        self.f32(rotation.y);
        self.f32(rotation.z);
        self.f32(rotation.s);
    }

    /// A list element, tagged by its type.
    fn element(&mut self, element: &Element) {
        match element {
            Element::Integer(integer) => {
                self.u8(0);
                self.i32(*integer);
            }
            Element::Float(float) => {
                self.u8(1);
                self.f32(*float);
            }
            Element::String(text) => {
                self.u8(2);
                self.str(text);
            }
            Element::Key(key) => {
                self.u8(3);
                self.str(key);
            }
            Element::Vector(vector) => {
                self.u8(4);
                self.vector(vector);
            }
            Element::Rotation(rotation) => {
                self.u8(5);
                self.rotation(rotation);
            }
        }
    }

    /// A value, tagged by its type: a list's tag is the one no element has.
    fn value(&mut self, value: &Value) {
        match value {
            Value::Integer(integer) => {
                self.u8(0);
                self.i32(*integer);
            }
            Value::Float(float) => {
                self.u8(1);
                self.f32(*float);
            }
            Value::String(text) => {
                self.u8(2);
                self.str(text);
            }
            Value::Key(key) => {
                self.u8(3);
                self.str(key);
            }
            Value::Vector(vector) => {
                self.u8(4);
                self.vector(vector);
            }
            Value::Rotation(rotation) => {
                self.u8(5);
                self.rotation(rotation);
            }
            Value::List(elements) => {
                self.u8(6);
                self.count(elements.len());
                for element in elements {
                    self.element(element);
                }
            }
        }
    }

    /// Values, counted.
    fn values(&mut self, values: &[Value]) {
        self.count(values.len());
        for value in values {
            self.value(value);
        }
    }

    /// A detected block, counted.
    fn detections(&mut self, detected: &[Detected]) {
        self.count(detected.len());
        for detection in detected {
            self.str(&detection.key);
            self.str(&detection.name);
            self.str(&detection.owner);
            self.i32(detection.kind);
            self.bool(detection.same_group);
            self.i32(detection.link_number);
            self.vector(&detection.position);
            self.rotation(&detection.rotation);
            self.vector(&detection.velocity);
            self.vector(&detection.grab);
            self.i32(detection.touch.face);
            self.vector(&detection.touch.st);
            self.vector(&detection.touch.uv);
            self.vector(&detection.touch.position);
            self.vector(&detection.touch.normal);
            self.vector(&detection.touch.binormal);
        }
    }

    /// The body in progress.
    fn activity(&mut self, activity: &SavedActivity) {
        match activity.phase {
            SavedPhase::Init => self.u8(0),
            SavedPhase::Event => self.u8(1),
            SavedPhase::Entry => self.u8(2),
            SavedPhase::Exit(target) => {
                self.u8(3);
                self.u32(target.0);
            }
        }
        self.count(activity.frames.len());
        for frame in &activity.frames {
            match frame.body {
                SavedBody::Init => self.u8(0),
                SavedBody::Function(id) => {
                    self.u8(1);
                    self.u32(id.0);
                }
                SavedBody::Handler { state, index } => {
                    self.u8(2);
                    self.u32(state.0);
                    self.u32(index);
                }
            }
            self.u32(frame.pc);
            self.values(&frame.locals);
            self.count(frame.costs.len());
            for cost in &frame.costs {
                self.u32(*cost);
            }
        }
        self.count(activity.stack.len());
        for operand in &activity.stack {
            match operand {
                Operand::Value(value) => {
                    self.u8(0);
                    self.value(value);
                }
                Operand::Double(double) => {
                    self.u8(1);
                    self.u64(double.to_bits());
                }
            }
        }
    }

    /// The listens, in the order they were opened.
    fn listens(&mut self, listens: &Listens) {
        self.count(listens.listens.len());
        for listen in &listens.listens {
            self.i32(listen.handle);
            self.i32(listen.filter.channel);
            self.str(&listen.filter.name);
            self.opt_str(listen.filter.speaker.as_deref());
            self.str(&listen.filter.message);
            self.i32(listen.filter.regex);
            self.bool(listen.active);
        }
    }

    /// The permissions and the questions waiting.
    fn permissions(&mut self, permissions: &Permissions) {
        self.opt_str(permissions.granter.as_deref());
        self.i32(permissions.mask);
        match &permissions.asked {
            Some(question) => {
                self.bool(true);
                self.str(&question.agent);
                self.i32(question.mask);
            }
            None => self.bool(false),
        }
        match &permissions.asked_experience {
            Some(question) => {
                self.bool(true);
                self.str(&question.agent);
                self.str(&question.experience);
                self.i32(question.mask);
            }
            None => self.bool(false),
        }
    }
}

/// The bytes being read, and how far.
struct Reader<'bytes> {
    /// All of them.
    bytes: &'bytes [u8],
    /// The next one to read.
    at: usize,
}

impl Reader<'_> {
    /// The next `count` bytes.
    fn take(&mut self, count: usize) -> Result<&[u8], DecodeError> {
        let end = self.at.checked_add(count).ok_or(DecodeError::Truncated)?;
        let taken = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
        self.at = end;
        Ok(taken)
    }

    /// The next `N` bytes, as an array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        self.take(N)?
            .try_into()
            .map_err(|_mismatch| DecodeError::Truncated)
    }

    /// One byte.
    fn u8(&mut self) -> Result<u8, DecodeError> {
        let [byte] = self.array::<1>()?;
        Ok(byte)
    }

    /// A flag.
    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(DecodeError::Tag { what: "flag", tag }),
        }
    }

    /// An unsigned 32-bit integer.
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(le::u32_from(self.array()?))
    }

    /// A signed 32-bit integer.
    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(le::i32_from(self.array()?))
    }

    /// An unsigned 64-bit integer.
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(le::u64_from(self.array()?))
    }

    /// A count.
    fn count(&mut self) -> Result<u32, DecodeError> {
        self.u32()
    }

    /// A float.
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_bits(self.u32()?))
    }

    /// A length of time.
    fn duration(&mut self) -> Result<Duration, DecodeError> {
        Ok(Duration::from_nanos(self.u64()?))
    }

    /// Text.
    fn str(&mut self) -> Result<String, DecodeError> {
        let length = usize::try_from(self.count()?).map_err(|_overflow| DecodeError::Truncated)?;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_invalid| DecodeError::Text)
    }

    /// Text that may be absent.
    fn opt_str(&mut self) -> Result<Option<String>, DecodeError> {
        if self.bool()? {
            self.str().map(Some)
        } else {
            Ok(None)
        }
    }

    /// A state that may be absent.
    fn state(&mut self) -> Result<Option<StateId>, DecodeError> {
        if self.bool()? {
            Ok(Some(StateId(self.u32()?)))
        } else {
            Ok(None)
        }
    }

    /// A vector.
    fn vector(&mut self) -> Result<Vector, DecodeError> {
        Ok(Vector {
            x: self.f32()?,
            y: self.f32()?,
            z: self.f32()?,
        })
    }

    /// A rotation.
    fn rotation(&mut self) -> Result<Rotation, DecodeError> {
        Ok(Rotation {
            x: self.f32()?,
            y: self.f32()?,
            z: self.f32()?,
            s: self.f32()?,
        })
    }

    /// A list element, after its tag.
    fn element_after(&mut self, tag: u8) -> Result<Element, DecodeError> {
        Ok(match tag {
            0 => Element::Integer(self.i32()?),
            1 => Element::Float(self.f32()?),
            2 => Element::String(self.str()?),
            3 => Element::Key(self.str()?),
            4 => Element::Vector(self.vector()?),
            5 => Element::Rotation(self.rotation()?),
            tag => {
                return Err(DecodeError::Tag {
                    what: "list element",
                    tag,
                });
            }
        })
    }

    /// A value.
    fn value(&mut self) -> Result<Value, DecodeError> {
        let tag = self.u8()?;
        if tag == 6 {
            let mut elements = Vec::new();
            for _ in 0..self.count()? {
                let tag = self.u8()?;
                elements.push(self.element_after(tag)?);
            }
            return Ok(Value::List(elements));
        }
        Ok(match self.element_after(tag)? {
            Element::Integer(integer) => Value::Integer(integer),
            Element::Float(float) => Value::Float(float),
            Element::String(text) => Value::String(text),
            Element::Key(key) => Value::Key(key),
            Element::Vector(vector) => Value::Vector(vector),
            Element::Rotation(rotation) => Value::Rotation(rotation),
        })
    }

    /// Values, counted.
    fn values(&mut self) -> Result<Vec<Value>, DecodeError> {
        let mut values = Vec::new();
        for _ in 0..self.count()? {
            values.push(self.value()?);
        }
        Ok(values)
    }

    /// A detected block, counted.
    fn detections(&mut self) -> Result<Vec<Detected>, DecodeError> {
        let mut detected = Vec::new();
        for _ in 0..self.count()? {
            detected.push(Detected {
                key: self.str()?,
                name: self.str()?,
                owner: self.str()?,
                kind: self.i32()?,
                same_group: self.bool()?,
                link_number: self.i32()?,
                position: self.vector()?,
                rotation: self.rotation()?,
                velocity: self.vector()?,
                grab: self.vector()?,
                touch: Touch {
                    face: self.i32()?,
                    st: self.vector()?,
                    uv: self.vector()?,
                    position: self.vector()?,
                    normal: self.vector()?,
                    binormal: self.vector()?,
                },
            });
        }
        Ok(detected)
    }

    /// The body in progress.
    fn activity(&mut self) -> Result<SavedActivity, DecodeError> {
        let phase = match self.u8()? {
            0 => SavedPhase::Init,
            1 => SavedPhase::Event,
            2 => SavedPhase::Entry,
            3 => SavedPhase::Exit(StateId(self.u32()?)),
            tag => return Err(DecodeError::Tag { what: "phase", tag }),
        };
        let mut frames = Vec::new();
        for _ in 0..self.count()? {
            let body = match self.u8()? {
                0 => SavedBody::Init,
                1 => SavedBody::Function(FunctionId(self.u32()?)),
                2 => SavedBody::Handler {
                    state: StateId(self.u32()?),
                    index: self.u32()?,
                },
                tag => return Err(DecodeError::Tag { what: "body", tag }),
            };
            let pc = self.u32()?;
            let locals = self.values()?;
            let mut costs = Vec::new();
            for _ in 0..self.count()? {
                costs.push(self.u32()?);
            }
            frames.push(SavedFrame {
                body,
                pc,
                locals,
                costs,
            });
        }
        let mut stack = Vec::new();
        for _ in 0..self.count()? {
            stack.push(match self.u8()? {
                0 => Operand::Value(self.value()?),
                1 => Operand::Double(f64::from_bits(self.u64()?)),
                tag => {
                    return Err(DecodeError::Tag {
                        what: "operand",
                        tag,
                    });
                }
            });
        }
        Ok(SavedActivity {
            phase,
            frames,
            stack,
        })
    }

    /// The listens.
    fn listens(&mut self) -> Result<Listens, DecodeError> {
        let mut listens = Vec::new();
        for _ in 0..self.count()? {
            let handle = self.i32()?;
            let filter = ListenFilter {
                channel: self.i32()?,
                name: self.str()?,
                speaker: self.opt_str()?,
                message: self.str()?,
                regex: self.i32()?,
            };
            listens.push(Listen {
                handle,
                filter,
                active: self.bool()?,
            });
        }
        Ok(Listens { listens })
    }

    /// The permissions and the questions waiting.
    fn permissions(&mut self) -> Result<Permissions, DecodeError> {
        let granter = self.opt_str()?;
        let mask = self.i32()?;
        let asked = if self.bool()? {
            Some(Question {
                agent: self.str()?,
                mask: self.i32()?,
            })
        } else {
            None
        };
        let asked_experience = if self.bool()? {
            Some(ExperienceQuestion {
                agent: self.str()?,
                experience: self.str()?,
                mask: self.i32()?,
            })
        } else {
            None
        };
        Ok(Permissions {
            granter,
            mask,
            asked,
            asked_experience,
        })
    }
}
