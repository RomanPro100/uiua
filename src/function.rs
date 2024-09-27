use std::{
    cmp::Ordering,
    collections::HashSet,
    fmt,
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, BitAnd, BitOr, BitOrAssign},
};

use ecow::{eco_vec, EcoString, EcoVec};
use enum_iterator::Sequence;
use serde::*;
use serde_tuple::*;

use crate::{
    check::instrs_signature,
    lex::CodeSpan,
    primitive::{ImplPrimitive, Primitive},
    value::Value,
    Assembly, BindingKind, Ident, NewFunction,
};

/// A Uiua bytecode instruction
#[derive(Clone)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum Instr {
    /// A comment
    Comment(Ident) = 0,
    /// Push a value onto the stack
    Push(Value),
    /// Push a global value onto the stack
    CallGlobal {
        index: usize,
        call: bool,
    },
    /// Bind a global value
    BindGlobal {
        span: usize,
        index: usize,
    },
    /// Begin an array
    BeginArray,
    /// End an array
    EndArray {
        boxed: bool,
        span: usize,
    },
    /// Execute a primitive
    Prim(Primitive, usize),
    /// Execute an implementation primitive
    ImplPrim(ImplPrimitive, usize),
    /// Call a function
    Call(usize),
    /// Call a recursive function
    CallRecursive(usize),
    /// Recur
    Recur(usize),
    /// Push a function onto the function stack
    PushFunc(Function),
    /// Set the positional macro arguments
    SetPosArgs {
        count: usize,
        span: usize,
    },
    /// Call a positional macro argument
    PushPosArg {
        index: usize,
        sig: Signature,
        span: usize,
    },
    /// Execute a switch
    Switch {
        count: usize,
        sig: Signature,
        span: usize,
        under_cond: bool,
    },
    /// Do a format string
    Format {
        parts: EcoVec<EcoString>,
        span: usize,
    },
    /// Match a format string pattern
    MatchFormatPattern {
        parts: EcoVec<EcoString>,
        span: usize,
    },
    /// Execute a stack swizzle
    StackSwizzle(StackSwizzle, usize),
    /// Label an array
    Label {
        label: EcoString,
        span: usize,
        remove: bool,
    },
    /// Validate a field type
    ValidateType {
        index: usize,
        name: EcoString,
        type_num: u8,
        span: usize,
    },
    /// Call a dynamic function
    Dynamic(DynamicFunction),
    Unpack {
        count: usize,
        span: usize,
        unbox: bool,
    },
    TouchStack {
        count: usize,
        span: usize,
    },
    PushTemp {
        stack: TempStack,
        count: usize,
        span: usize,
    },
    PopTemp {
        stack: TempStack,
        count: usize,
        span: usize,
    },
    CopyToTemp {
        stack: TempStack,
        count: usize,
        span: usize,
    },
    SetOutputComment {
        i: usize,
        n: usize,
    },
    PushSig(Signature),
    PopSig,
}

/// A type of temporary stacks
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Sequence, Serialize, Deserialize,
)]
pub enum TempStack {
    /// A stack used to hold values need to undo a function
    #[serde(rename = "u")]
    Under,
    /// A stack used when inlining some functions
    #[serde(rename = "i")]
    Inline,
}

impl fmt::Display for TempStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Under => write!(f, "under"),
            Self::Inline => write!(f, "inline"),
        }
    }
}

impl PartialEq for Instr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Push(a), Self::Push(b)) => a == b,
            (Self::BeginArray, Self::BeginArray) => true,
            (Self::EndArray { boxed: a, .. }, Self::EndArray { boxed: b, .. }) => a == b,
            (Self::Prim(a, _), Self::Prim(b, _)) => a == b,
            (Self::ImplPrim(a, _), Self::ImplPrim(b, _)) => a == b,
            (Self::Call(a), Self::Call(b)) => a == b,
            (Self::Format { parts: a, .. }, Self::Format { parts: b, .. }) => a == b,
            (Self::MatchFormatPattern { parts: a, .. }, Self::Format { parts: b, .. }) => a == b,
            (Self::PushFunc(a), Self::PushFunc(b)) => a == b,
            (Self::PushTemp { count: a, .. }, Self::PushTemp { count: b, .. }) => a == b,
            (Self::PopTemp { count: a, .. }, Self::PopTemp { count: b, .. }) => a == b,
            (Self::TouchStack { count: a, .. }, Self::TouchStack { count: b, .. }) => a == b,
            (Self::Comment(a), Self::Comment(b)) => a == b,
            (Self::CallGlobal { index: a, .. }, Self::CallGlobal { index: b, .. }) => a == b,
            (Self::BindGlobal { index: a, .. }, Self::BindGlobal { index: b, .. }) => a == b,
            (
                Self::Switch {
                    count: a,
                    under_cond: au,
                    sig: asig,
                    ..
                },
                Self::Switch {
                    count: b,
                    under_cond: bu,
                    sig: bsig,
                    ..
                },
            ) => a == b && au == bu && asig == bsig,
            (Self::Label { label: a, .. }, Self::Label { label: b, .. }) => a == b,
            (Self::Dynamic(a), Self::Dynamic(b)) => a == b,
            (
                Self::Unpack {
                    count: a,
                    unbox: au,
                    ..
                },
                Self::Unpack {
                    count: b,
                    unbox: bu,
                    ..
                },
            ) => a == b && au == bu,
            (Self::SetOutputComment { i: ai, n: an }, Self::SetOutputComment { i: bi, n: bn }) => {
                ai == bi && an == bn
            }
            (Self::PushSig(a), Self::PushSig(b)) => a == b,
            (Self::PopSig, Self::PopSig) => true,
            (Self::StackSwizzle(a, _), Self::StackSwizzle(b, _)) => a == b,
            (
                Self::ValidateType {
                    index: ai,
                    name: a,
                    type_num: an,
                    ..
                },
                Self::ValidateType {
                    index: bi,
                    name: b,
                    type_num: bn,
                    ..
                },
            ) => ai == bi && a == b && an == bn,
            _ => false,
        }
    }
}

impl Eq for Instr {}

impl Hash for Instr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Instr::Push(val) => (0, val).hash(state),
            Instr::BeginArray => 1.hash(state),
            Instr::EndArray { boxed, .. } => (2, boxed).hash(state),
            Instr::Prim(prim, _) => (3, prim).hash(state),
            Instr::ImplPrim(prim, _) => (4, prim).hash(state),
            Instr::Call(_) => 5.hash(state),
            Instr::CallRecursive(_) => 29.hash(state),
            Instr::Recur(_) => 30.hash(state),
            Instr::Format { parts, .. } => (6, parts).hash(state),
            Instr::MatchFormatPattern { parts, .. } => (28, parts).hash(state),
            Instr::PushFunc(func) => (7, func).hash(state),
            Instr::PushTemp { count, stack, .. } => (8, count, stack).hash(state),
            Instr::PopTemp { count, stack, .. } => (9, count, stack).hash(state),
            Instr::CopyToTemp { count, stack, .. } => (10, count, stack).hash(state),
            Instr::TouchStack { count, .. } => (13, count).hash(state),
            Instr::Comment(_) => (14, 0).hash(state),
            Instr::CallGlobal { index, call } => (15, index, call).hash(state),
            Instr::BindGlobal { index, .. } => (16, index).hash(state),
            Instr::Switch {
                count,
                under_cond,
                sig,
                ..
            } => (17, count, under_cond, sig).hash(state),
            Instr::Label { label, remove, .. } => (18, label, remove).hash(state),
            Instr::Dynamic(df) => (19, df).hash(state),
            Instr::Unpack { count, unbox, .. } => (23, count, unbox).hash(state),
            Instr::SetOutputComment { i, n, .. } => (24, i, n).hash(state),
            Instr::PushSig(sig) => (25, sig).hash(state),
            Instr::PopSig => 26.hash(state),
            Instr::ValidateType {
                index,
                name,
                type_num,
                ..
            } => (27, index, name, type_num).hash(state),
            Instr::StackSwizzle(swizzle, _) => (31, swizzle).hash(state),
            Instr::SetPosArgs { count, .. } => (32, count).hash(state),
            Instr::PushPosArg { index, .. } => (33, index).hash(state),
        }
    }
}

impl Instr {
    /// Create a new push instruction
    pub fn push(val: impl Into<Value>) -> Self {
        let val = val.into();
        Self::Push(val)
    }
    pub(crate) fn push_inline(count: usize, span: usize) -> Self {
        Self::PushTemp {
            stack: TempStack::Inline,
            count,
            span,
        }
    }
    pub(crate) fn pop_inline(count: usize, span: usize) -> Self {
        Self::PopTemp {
            stack: TempStack::Inline,
            count,
            span,
        }
    }
    pub(crate) fn copy_inline(span: usize) -> Self {
        Self::CopyToTemp {
            stack: TempStack::Inline,
            count: 1,
            span,
        }
    }
    pub(crate) fn is_compile_only(&self) -> bool {
        matches!(self, Self::PushSig(_) | Self::PopSig)
    }
    #[allow(dead_code)]
    pub(crate) fn span(&self) -> Option<usize> {
        self.span_impl().copied()
    }
}

macro_rules! instr_span {
    ($name:ident, $self_ty:ty, $out_ty:ty) => {
        impl Instr {
            #[allow(dead_code)]
            pub(crate) fn $name(self: $self_ty) -> Option<$out_ty> {
                Some(match self {
                    Self::BindGlobal { span, .. } => span,
                    Self::EndArray { span, .. } => span,
                    Self::Call(span) => span,
                    Self::Prim(_, span) => span,
                    Self::ImplPrim(_, span) => span,
                    Self::CallRecursive(span) => span,
                    Self::Recur(span) => span,
                    Self::Switch { span, .. } => span,
                    Self::Format { span, .. } => span,
                    Self::MatchFormatPattern { span, .. } => span,
                    Self::PushTemp { span, .. } => span,
                    Self::PopTemp { span, .. } => span,
                    Self::CopyToTemp { span, .. } => span,
                    Self::Label { span, .. } => span,
                    Self::ValidateType { span, .. } => span,
                    Self::Unpack { span, .. } => span,
                    Self::TouchStack { span, .. } => span,
                    Self::SetPosArgs { span, .. } => span,
                    _ => return None,
                })
            }
        }
    };
}

instr_span!(span_impl, &Self, &usize);
instr_span!(span_mut, &mut Self, &mut usize);

pub(crate) struct FmtInstrs<'a>(pub &'a [Instr], pub &'a Assembly);
impl<'a> fmt::Debug for FmtInstrs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        for (i, instr) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            match instr {
                Instr::PushFunc(func) => {
                    FmtInstrs(func.instrs(self.1), self.1).fmt(f)?;
                }
                instr => instr.fmt(f)?,
            }
        }
        write!(f, ")")?;
        Ok(())
    }
}
impl<'a> fmt::Display for FmtInstrs<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, instr) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            match instr {
                Instr::PushFunc(func) => FmtInstrs(func.instrs(self.1), self.1).fmt(f),
                instr => instr.fmt(f),
            }?
        }
        Ok(())
    }
}

/// A swizzle for the stack
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct StackSwizzle {
    /// The indices of the stack elements
    pub indices: EcoVec<u8>,
    /// The fix mask
    pub fix: EcoVec<bool>,
}

impl fmt::Display for StackSwizzle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "λ")?;
        for (&i, &fix) in self.indices.iter().zip(&self.fix) {
            let mut c = b'a' + i;
            if fix {
                c = c.to_ascii_uppercase();
            }
            write!(f, "{}", c as char)?;
        }
        Ok(())
    }
}

impl StackSwizzle {
    pub(crate) fn args(&self) -> usize {
        (self.indices.iter().max().copied()).map_or(0, |max| max as usize + 1)
    }
    /// Get the signature of the swizzle
    pub fn signature(&self) -> Signature {
        Signature::new(self.args(), self.indices.len())
    }
    /// Get the inverse of the swizzle
    pub fn inverse(&self) -> Option<Self> {
        if self.args() != self.indices.len() {
            return None;
        }
        let set: HashSet<_> = self.indices.iter().copied().collect();
        if set.len() != self.indices.len() {
            return None;
        }
        let mut indices = eco_vec![0; self.indices.len()];
        let slice = indices.make_mut();
        for (i, &j) in self.indices.iter().enumerate() {
            slice[j as usize] = i as u8;
        }
        Some(Self {
            indices,
            fix: self.fix.clone(),
        })
    }
}

/// A swizzle for an array
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct ArraySwizzle {
    /// The indices of the array elements
    pub indices: EcoVec<i8>,
    /// The (un)box mask
    pub unbox: EcoVec<bool>,
}

impl fmt::Display for ArraySwizzle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "⋊")?;
        for (&i, &b) in self.indices.iter().zip(&self.unbox) {
            let mut c = if i < 0 {
                b'z' + 1 - i.unsigned_abs()
            } else {
                b'a' + i.unsigned_abs()
            };
            if b {
                c = c.to_ascii_uppercase();
            }
            write!(f, "{}", c as char)?;
        }
        Ok(())
    }
}

impl ArraySwizzle {
    /// Get the signature of the swizzle
    pub fn signature(&self) -> Signature {
        Signature::new(1, self.indices.len())
    }
}

/// Levels of purity for an operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Purity {
    /// The operation visibly affects the environment
    Mutating,
    /// The operation reads from the environment but does not visibly affect it
    Impure,
    /// The operation is completely pure
    Pure,
}

/// Whether some instructions are pure
pub(crate) fn instrs_are_pure(instrs: &[Instr], asm: &Assembly, min_purity: Purity) -> bool {
    'instrs: for (i, instr) in instrs.iter().enumerate() {
        match instr {
            Instr::CallGlobal { index, .. } => {
                if let Some(binding) = asm.bindings.get(*index) {
                    match &binding.kind {
                        BindingKind::Const(Some(_)) => {}
                        BindingKind::Func(f) => {
                            if !instrs_are_pure(f.instrs(asm), asm, min_purity) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
            }
            Instr::BindGlobal { .. } => {
                let prev = &instrs[..i];
                for j in (0..i).rev() {
                    let frag = &prev[j..i];
                    if instrs_signature(frag).is_ok_and(|sig| sig == (0, 1))
                        && instrs_are_pure(frag, asm, min_purity)
                    {
                        continue 'instrs;
                    }
                }
                return false;
            }
            Instr::Prim(prim, _) => {
                if prim.purity() < min_purity {
                    return false;
                }
            }
            Instr::ImplPrim(prim, _) => {
                if prim.purity() < min_purity {
                    return false;
                }
            }
            Instr::PushFunc(f) => {
                if !instrs_are_pure(f.instrs(asm), asm, min_purity) {
                    return false;
                }
            }
            Instr::Dynamic(_) => return false,
            Instr::SetOutputComment { .. } => return false,
            _ => {}
        }
    }
    true
}

/// Whether some instructions can be propertly bounded by the runtime execution limit
pub(crate) fn instrs_are_limit_bounded(instrs: &[Instr], asm: &Assembly) -> bool {
    use Primitive::*;
    for instr in instrs {
        match instr {
            Instr::CallGlobal { index, .. } => {
                if let Some(binding) = asm.bindings.get(*index) {
                    match &binding.kind {
                        BindingKind::Const(Some(_)) => {}
                        BindingKind::Func(f) => {
                            if !instrs_are_limit_bounded(f.instrs(asm), asm) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
            }
            Instr::Prim(Send | Recv, _) => return false,
            Instr::Prim(Sys(op), _) if op.purity() <= Purity::Mutating => return false,
            Instr::PushFunc(f) => {
                if f.is_recursive() || !instrs_are_limit_bounded(f.instrs(asm), asm) {
                    return false;
                }
            }
            Instr::Dynamic(_) => return false,
            _ => {}
        }
    }
    true
}

impl fmt::Debug for Instr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instr::Push(val) => {
                if val.element_count() < 50 && val.shape().len() <= 1 {
                    write!(f, "push {val:?}")
                } else {
                    write!(f, "push {} array", val.shape())
                }
            }
            _ => write!(f, "{self}"),
        }
    }
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instr::Comment(comment) => write!(f, "# {comment}"),
            Instr::Push(val) => write!(f, "{val:?}"),
            Instr::CallGlobal { index, .. } => write!(f, "<call global {index}>"),
            Instr::BindGlobal { index, .. } => write!(f, "<bind global {index}>"),
            Instr::BeginArray => write!(f, "begin array"),
            Instr::EndArray { boxed: false, .. } => write!(f, "end array"),
            Instr::EndArray { boxed: true, .. } => write!(f, "end box array"),
            Instr::Prim(prim @ Primitive::Over, _) => write!(f, "`{prim}`"),
            Instr::Prim(prim, _) => write!(f, "{prim}"),
            Instr::ImplPrim(prim, _) => write!(f, "{prim}"),
            Instr::Call(_) => write!(f, "call"),
            Instr::CallRecursive(_) => write!(f, "call recursive"),
            Instr::Recur(_) => write!(f, "recur"),
            Instr::PushFunc(func) => write!(f, "push({func})"),
            Instr::Switch { count, .. } => write!(f, "<switch {count}>"),
            Instr::Format { parts, .. } => {
                write!(f, "$\"")?;
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, "_")?
                    }
                    write!(f, "{part}")?
                }
                write!(f, "\"")
            }
            Instr::MatchFormatPattern { parts, .. } => {
                write!(f, "°$\"")?;
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, "_")?
                    }
                    write!(f, "{part}")?
                }
                write!(f, "\"")
            }
            Instr::Label { label, .. } => write!(f, "${label}"),
            Instr::ValidateType { name, type_num, .. } => {
                write!(f, "<validate {name} as {type_num}>")
            }
            Instr::Dynamic(df) => write!(f, "{df:?}"),
            Instr::Unpack {
                count,
                unbox: false,
                ..
            } => write!(f, "<unpack {count}>"),
            Instr::Unpack {
                count, unbox: true, ..
            } => write!(f, "<unpack (unbox) {count}>"),
            Instr::TouchStack { count, .. } => write!(f, "<touch {count}>"),
            Instr::PushTemp { stack, count, .. } => write!(f, "<push {stack} {count}>"),
            Instr::PopTemp { stack, count, .. } => write!(f, "<pop {stack} {count}>"),
            Instr::CopyToTemp { stack, count, .. } => {
                write!(f, "<copy to {stack} {count}>")
            }
            Instr::StackSwizzle(swizzle, _) => write!(f, "{swizzle}"),
            Instr::SetOutputComment { i, n, .. } => write!(f, "<set output comment {i}({n})>"),
            Instr::PushSig(sig) => write!(f, "{sig}"),
            Instr::PopSig => write!(f, "-|"),
            Instr::SetPosArgs { count, .. } => write!(f, "<set {count} pos args>"),
            Instr::PushPosArg { index, .. } => write!(f, "<call pos arg {index}>"),
        }
    }
}

/// A Uiua function
#[derive(Clone, Serialize_tuple, Deserialize_tuple)]
pub struct Function {
    /// The function's id
    pub id: FunctionId,
    signature: Signature,
    pub(crate) slice: FuncSlice,
    hash: u64,
    pub(crate) flags: FunctionFlags,
}

impl Default for Function {
    fn default() -> Self {
        Self {
            id: FunctionId::Unnamed,
            signature: Signature::new(0, 0),
            slice: FuncSlice::default(),
            hash: 0,
            flags: FunctionFlags::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct FunctionFlags(u8);

impl BitOrAssign for FunctionFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitOr for FunctionFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for FunctionFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl FunctionFlags {
    pub const RECURSIVE: Self = Self(1 << 0);
    pub const NO_INLINE: Self = Self(1 << 1);
    pub const TRACK_CALLER: Self = Self(1 << 2);
    pub const NO_PRE_EVAL: Self = Self(1 << 3);
    pub fn recursive(&self) -> bool {
        self.0 & Self::RECURSIVE.0 != 0
    }
    pub fn no_inline(&self) -> bool {
        self.0 & Self::NO_INLINE.0 != 0
    }
    pub fn track_caller(&self) -> bool {
        self.0 & Self::TRACK_CALLER.0 != 0
    }
    pub fn no_pre_eval(&self) -> bool {
        self.0 & Self::NO_PRE_EVAL.0 != 0
    }
}

/// A range of compiled instructions
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(from = "(usize, usize)", into = "(usize, usize)")]
pub struct FuncSlice {
    pub(crate) start: usize,
    pub(crate) len: usize,
}

impl From<(usize, usize)> for FuncSlice {
    fn from((start, len): (usize, usize)) -> Self {
        Self { start, len }
    }
}

impl From<FuncSlice> for (usize, usize) {
    fn from(slice: FuncSlice) -> Self {
        (slice.start, slice.len)
    }
}

impl FuncSlice {
    /// Get the length of the instructions
    pub fn len(&self) -> usize {
        self.len
    }
    /// Check if the instructions are empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Get the end of the instructions
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

impl AddAssign<usize> for FuncSlice {
    fn add_assign(&mut self, rhs: usize) {
        self.start += rhs;
    }
}

impl Add<usize> for FuncSlice {
    type Output = Self;
    fn add(mut self, rhs: usize) -> Self::Output {
        self += rhs;
        self
    }
}

/// A function stack signature
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "(usize, usize)", into = "(usize, usize)")]
pub struct Signature {
    /// The number of arguments the function pops off the stack
    pub args: usize,
    /// The number of values the function pushes onto the stack
    pub outputs: usize,
}

impl From<(usize, usize)> for Signature {
    fn from((args, outputs): (usize, usize)) -> Self {
        Self::new(args, outputs)
    }
}

impl From<Signature> for (usize, usize) {
    fn from(sig: Signature) -> Self {
        (sig.args, sig.outputs)
    }
}

impl Signature {
    /// Create a new signature with the given number of arguments and outputs
    pub const fn new(args: usize, outputs: usize) -> Self {
        Self { args, outputs }
    }
    /// Check if this signature changes the stack size by the same amount as another signature
    pub fn is_compatible_with(self, other: Self) -> bool {
        self.args as isize - self.outputs as isize == other.args as isize - other.outputs as isize
    }
    /// Check if this [`Signature::is_compatible_with`] another signature and has at least as many arguments
    pub fn is_superset_of(self, other: Self) -> bool {
        self.is_compatible_with(other) && self.args >= other.args
    }
    /// Check if this [`Signature::is_compatible_with`] another signature and has at most as many arguments
    pub fn is_subset_of(self, other: Self) -> bool {
        self.is_compatible_with(other) && self.args <= other.args
    }
    /// Get the signature that has the maximum of the arguments and outputs of this signature and another
    pub fn max_with(self, other: Self) -> Self {
        Self::new(self.args.max(other.args), self.outputs.max(other.outputs))
    }
    /// Compose signatures as if a function with signature `other` was called before a function with signature `self`
    pub fn compose(self, other: Self) -> Self {
        let args = other.args + self.args.saturating_sub(other.outputs);
        let outputs = self.outputs + other.outputs.saturating_sub(self.args);
        Self::new(args, outputs)
    }
    /// Get the inverse of this signature
    pub fn inverse(self) -> Self {
        Self::new(self.outputs, self.args)
    }
}

impl PartialEq<(usize, usize)> for Signature {
    fn eq(&self, other: &(usize, usize)) -> bool {
        self.args == other.0 && self.outputs == other.1
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "|{}.{}", self.args, self.outputs)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.outputs == 1 {
            write!(f, "|{}", self.args)
        } else {
            write!(f, "{self:?}")
        }
    }
}

/// A function that executes Rust code
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DynamicFunction {
    /// An index used to look up the function
    pub(crate) index: usize,
    /// The function's signature
    pub(crate) signature: Signature,
}

impl From<(usize, Signature)> for DynamicFunction {
    fn from((index, signature): (usize, Signature)) -> Self {
        Self { index, signature }
    }
}

impl From<DynamicFunction> for (usize, Signature) {
    fn from(func: DynamicFunction) -> Self {
        (func.index, func.signature)
    }
}

impl DynamicFunction {
    /// Get the function's signature
    pub fn signature(&self) -> Signature {
        self.signature
    }
}

impl fmt::Debug for DynamicFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<dynamic#{:x}>", self.index)
    }
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.slice == other.slice && self.hash == other.hash
    }
}

impl Eq for Function {}

impl PartialOrd for Function {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Function {
    fn cmp(&self, other: &Self) -> Ordering {
        self.slice.cmp(&other.slice)
    }
}

impl Hash for Function {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.slice.hash(state);
        self.hash.hash(state);
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl Function {
    /// Create a new function
    pub(crate) fn new(id: FunctionId, signature: Signature, slice: FuncSlice, hash: u64) -> Self {
        Self {
            id,
            slice,
            signature,
            hash,
            flags: FunctionFlags::default(),
        }
    }
    pub(crate) fn with_flags(mut self, flags: FunctionFlags) -> Self {
        self.flags = flags;
        self
    }
    /// Get the [`Signature`] of this function
    pub fn signature(&self) -> Signature {
        self.signature
    }
    /// Whether this function is recursive
    pub fn is_recursive(&self) -> bool {
        self.flags.recursive()
    }
    /// Get the address of function's instructions
    pub fn slice(&self) -> FuncSlice {
        self.slice
    }
    /// Get the function's instructions
    #[track_caller]
    pub fn instrs<'a>(&self, asm: &'a Assembly) -> &'a [Instr] {
        let end = self.slice.end();
        assert!(
            self.slice.start <= asm.instrs.len(),
            "{self} slice start {} out of bounds of {} instrs",
            self.slice.start,
            asm.instrs.len()
        );
        assert!(
            end <= asm.instrs.len(),
            "{self} slice end {} out of bounds of {} instrs",
            end,
            asm.instrs.len()
        );
        &asm.instrs[self.slice.start..end]
    }
    pub(crate) fn new_func(&self, asm: &Assembly) -> NewFunction {
        NewFunction {
            instrs: self.instrs(asm).into(),
            flags: self.flags,
        }
    }
    /// Get a mutable slice of the function's instructions
    pub fn instrs_mut<'a>(&self, asm: &'a mut Assembly) -> &'a mut [Instr] {
        asm.instrs_mut(self.slice)
    }
    /// Try to get a lone primitive from this function
    pub fn as_primitive(&self, asm: &Assembly) -> Option<Primitive> {
        self.as_flipped_primitive(asm)
            .filter(|(_, flipped)| !flipped)
            .map(|(prim, _)| prim)
    }
    /// Try to get a lone implementation primitive from this function
    pub fn as_impl_primitive(&self, asm: &Assembly) -> Option<ImplPrimitive> {
        self.as_flipped_impl_primitive(asm)
            .filter(|(_, flipped)| !flipped)
            .map(|(prim, _)| prim)
    }
    pub(crate) fn as_flipped_primitive(&self, asm: &Assembly) -> Option<(Primitive, bool)> {
        match &self.id {
            FunctionId::Primitive(prim) => Some((*prim, false)),
            _ => instrs_as_flipped_primitive(self.instrs(asm), asm),
        }
    }
    pub(crate) fn as_flipped_impl_primitive(
        &self,
        asm: &Assembly,
    ) -> Option<(ImplPrimitive, bool)> {
        instrs_as_flipped_impl_primitive(self.instrs(asm), asm)
    }
    pub(crate) fn hash(&self) -> u64 {
        self.hash
    }
}

pub(crate) fn instrs_as_flipped_primitive(
    instrs: &[Instr],
    asm: &Assembly,
) -> Option<(Primitive, bool)> {
    use Primitive::*;
    match instrs {
        [Instr::Prim(Flip, _), Instr::Prim(prim, _)] => Some((*prim, true)),
        [Instr::Prim(prim, _)] => Some((*prim, false)),
        [Instr::PushFunc(f), Instr::Call(_)] => f.as_flipped_primitive(asm.as_ref()),
        _ => None,
    }
}
pub(crate) fn instrs_as_flipped_impl_primitive(
    instrs: &[Instr],
    asm: &Assembly,
) -> Option<(ImplPrimitive, bool)> {
    use Primitive::*;
    match instrs {
        [Instr::ImplPrim(prim, _), Instr::Prim(Flip, _)] => Some((*prim, true)),
        [Instr::ImplPrim(prim, _)] => Some((*prim, false)),
        [Instr::PushFunc(f), Instr::Call(_)] => f.as_flipped_impl_primitive(asm.as_ref()),
        _ => None,
    }
}

/// A Uiua function id
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FunctionId {
    /// Just a primitive
    Primitive(Primitive),
    /// A named function
    Named(Ident),
    /// An anonymous function
    Anonymous(CodeSpan),
    /// A macro expansion
    Macro(CodeSpan),
    /// The top-level function
    Main,
    #[doc(hidden)]
    /// Implementation detail
    Unnamed,
}

impl FunctionId {
    /// Get the span of the function id, if it has one
    pub fn span(&self) -> Option<&CodeSpan> {
        match self {
            FunctionId::Anonymous(span) => Some(span),
            FunctionId::Macro(span) => Some(span),
            _ => None,
        }
    }
}

impl PartialEq<&str> for FunctionId {
    fn eq(&self, other: &&str) -> bool {
        match self {
            FunctionId::Named(name) => &&**name == other,
            _ => false,
        }
    }
}

impl From<Ident> for FunctionId {
    fn from(name: Ident) -> Self {
        Self::Named(name)
    }
}

impl From<Primitive> for FunctionId {
    fn from(op: Primitive) -> Self {
        Self::Primitive(op)
    }
}

impl From<CodeSpan> for FunctionId {
    fn from(span: CodeSpan) -> Self {
        Self::Anonymous(span)
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FunctionId::Named(name) => write!(f, "{name}"),
            FunctionId::Anonymous(span) => write!(f, "fn from {span}"),
            FunctionId::Primitive(prim) => write!(f, "{prim}"),
            FunctionId::Macro(_) => write!(f, "macro expansion"),
            FunctionId::Main => write!(f, "main"),
            FunctionId::Unnamed => write!(f, "unnamed"),
        }
    }
}
