//! Algorithms for performing operations on arrays

use std::{
    cmp::Ordering,
    convert::Infallible,
    fmt,
    hash::{Hash, Hasher},
    iter,
    mem::size_of,
    option,
};

use tinyvec::TinyVec;

use crate::{
    Array, ArrayValue, CodeSpan, ExactDoubleIterator, Function, Inputs, PersistentMeta, Shape,
    Signature, Span, TempStack, Uiua, UiuaError, UiuaResult, Value,
};

mod dyadic;
pub(crate) mod invert;
pub mod loops;
pub(crate) mod map;
mod monadic;
pub mod pervade;
pub mod reduce;
pub mod table;
pub mod zip;

type MultiOutput<T> = TinyVec<[T; 1]>;
fn multi_output<T: Clone + Default>(n: usize, val: T) -> MultiOutput<T> {
    let mut vec = TinyVec::with_capacity(n);
    if n == 0 {
        return vec;
    }
    for _ in 0..n - 1 {
        vec.push(val.clone());
    }
    vec.push(val);
    vec
}

fn max_shape(a: &[usize], b: &[usize]) -> Shape {
    let shape_len = a.len().max(b.len());
    let mut new_shape = Shape::with_capacity(shape_len);
    for _ in 0..shape_len {
        new_shape.push(0);
    }
    for i in 0..new_shape.len() {
        let j = new_shape.len() - i - 1;
        if a.len() > i {
            new_shape[j] = a[a.len() - i - 1];
        }
        if b.len() > i {
            new_shape[j] = new_shape[j].max(b[b.len() - i - 1]);
        }
    }
    new_shape
}

#[derive(Debug)]
pub enum SizeError {
    Overflow,
    TooLarge(usize),
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SizeError::Overflow => write!(f, "Array size calculation overflowed"),
            SizeError::TooLarge(size) => write!(f, "Array of {size} elements would be too large"),
        }
    }
}

impl std::error::Error for SizeError {}

pub fn validate_size<T>(
    sizes: impl IntoIterator<Item = usize> + Clone,
    env: &Uiua,
) -> UiuaResult<usize> {
    validate_size_impl(size_of::<T>(), sizes).map_err(|e| env.error(e))
}

pub(crate) fn validate_size_impl(
    size: usize,
    sizes: impl IntoIterator<Item = usize> + Clone,
) -> Result<usize, SizeError> {
    if sizes.clone().into_iter().any(|s| s == 0) {
        return Ok(0);
    }
    let (elements, mut overflowed) = sizes.into_iter().fold((1usize, false), |(acc, ovf), s| {
        let (new_acc, new_ovf) = acc.overflowing_mul(s);
        (new_acc, ovf || new_ovf)
    });
    let elem_size = size;
    let (size, ovf) = elements.overflowing_mul(elem_size);
    overflowed |= ovf;
    if overflowed {
        return Err(SizeError::Overflow);
    }
    let max_mega = if cfg!(target_arch = "wasm32") {
        256
    } else {
        4096
    };
    if size > max_mega * 1024usize.pow(2) {
        return Err(SizeError::TooLarge(elements));
    }
    Ok(elements)
}

pub trait ErrorContext {
    type Error: FillError;
    fn error(&self, msg: impl ToString) -> Self::Error;
}

impl ErrorContext for Uiua {
    type Error = UiuaError;
    fn error(&self, msg: impl ToString) -> Self::Error {
        self.error(msg)
    }
}

impl ErrorContext for (&CodeSpan, &Inputs) {
    type Error = UiuaError;
    fn error(&self, msg: impl ToString) -> Self::Error {
        UiuaError::Run(
            Span::Code(self.0.clone()).sp(msg.to_string()),
            self.1.clone().into(),
        )
    }
}

impl ErrorContext for () {
    type Error = Infallible;
    fn error(&self, msg: impl ToString) -> Self::Error {
        panic!("{}", msg.to_string())
    }
}

pub struct IgnoreError;
impl ErrorContext for IgnoreError {
    type Error = ();
    fn error(&self, _: impl ToString) -> Self::Error {}
}

pub trait FillError: fmt::Debug {
    fn is_fill(&self) -> bool;
}

impl FillError for () {
    fn is_fill(&self) -> bool {
        false
    }
}

impl FillError for UiuaError {
    fn is_fill(&self) -> bool {
        UiuaError::is_fill(self)
    }
}

impl FillError for Infallible {
    fn is_fill(&self) -> bool {
        match *self {}
    }
}

pub trait FillContext: ErrorContext {
    fn scalar_fill<T: ArrayValue>(&self) -> Result<T, &'static str>;
    fn array_fill<T: ArrayValue>(&self) -> Result<Array<T>, &'static str>;
    fn fill_error(error: Self::Error) -> Self::Error;
    fn is_fill_error(error: &Self::Error) -> bool;
}

impl FillContext for Uiua {
    fn scalar_fill<T: ArrayValue>(&self) -> Result<T, &'static str> {
        T::get_scalar_fill(self)
    }
    fn array_fill<T: ArrayValue>(&self) -> Result<Array<T>, &'static str> {
        T::get_array_fill(self)
    }
    fn fill_error(error: Self::Error) -> Self::Error {
        error.fill()
    }
    fn is_fill_error(error: &Self::Error) -> bool {
        error.is_fill()
    }
}

impl FillContext for () {
    fn scalar_fill<T: ArrayValue>(&self) -> Result<T, &'static str> {
        Err(". No fill is set.")
    }
    fn array_fill<T: ArrayValue>(&self) -> Result<Array<T>, &'static str> {
        Err(". No fill is set.")
    }
    fn fill_error(error: Self::Error) -> Self::Error {
        error
    }
    fn is_fill_error(error: &Self::Error) -> bool {
        match *error {}
    }
}

impl FillContext for (&CodeSpan, &Inputs) {
    fn scalar_fill<T: ArrayValue>(&self) -> Result<T, &'static str> {
        Err(". No fill is set.")
    }
    fn array_fill<T: ArrayValue>(&self) -> Result<Array<T>, &'static str> {
        Err(". No fill is set.")
    }
    fn fill_error(error: Self::Error) -> Self::Error {
        error.fill()
    }
    fn is_fill_error(error: &Self::Error) -> bool {
        error.is_fill()
    }
}

pub(crate) fn shape_prefixes_match(a: &[usize], b: &[usize]) -> bool {
    a.iter().zip(b).all(|(a, b)| a == b)
}

fn fill_value_shape<C>(
    val: &mut Value,
    target: &Shape,
    expand_fixed: bool,
    ctx: &C,
) -> Result<(), C::Error>
where
    C: FillContext,
{
    match val {
        Value::Num(arr) => fill_array_shape(arr, target, expand_fixed, ctx),
        Value::Byte(arr) => {
            *val = op_bytes_retry_fill(
                arr.clone(),
                |mut arr| {
                    fill_array_shape(&mut arr, target, expand_fixed, ctx)?;
                    Ok(arr.into())
                },
                |mut arr| {
                    fill_array_shape(&mut arr, target, expand_fixed, ctx)?;
                    Ok(arr.into())
                },
            )?;
            Ok(())
        }
        Value::Complex(arr) => fill_array_shape(arr, target, expand_fixed, ctx),
        Value::Char(arr) => fill_array_shape(arr, target, expand_fixed, ctx),
        Value::Box(arr) => fill_array_shape(arr, target, expand_fixed, ctx),
    }
}

fn fill_array_shape<T, C>(
    arr: &mut Array<T>,
    target: &Shape,
    expand_fixed: bool,
    ctx: &C,
) -> Result<(), C::Error>
where
    T: ArrayValue,
    C: FillContext,
{
    if shape_prefixes_match(&arr.shape, target) {
        return Ok(());
    }
    if expand_fixed && arr.row_count() == 1 && ctx.scalar_fill::<T>().is_err() {
        let fixes = (arr.shape.iter())
            .take_while(|&&dim| dim == 1)
            .count()
            .min(arr.shape.len());
        let same_under_fixes = (target.iter().skip(fixes).rev())
            .zip(arr.shape[fixes..].iter().rev())
            .all(|(b, a)| b == a);
        if same_under_fixes {
            arr.shape.drain(..fixes);
            if target.len() >= fixes {
                for &dim in target.iter().take(fixes).rev() {
                    arr.reshape_scalar(Ok(dim as isize));
                }
            } else if arr.shape() == target {
                for &dim in target.iter().cycle().take(fixes) {
                    arr.reshape_scalar(Ok(dim as isize));
                }
            }
        }
        if shape_prefixes_match(&arr.shape, target) {
            return Ok(());
        }
    }
    // Fill in missing rows
    let target_row_count = target.first().copied().unwrap_or(1);
    let mut fill_error = None;
    match arr.row_count().cmp(&target_row_count) {
        Ordering::Less => match ctx.scalar_fill() {
            Ok(fill) => {
                let mut target_shape = arr.shape().to_vec();
                target_shape[0] = target_row_count;
                arr.fill_to_shape(&target_shape, fill);
            }
            Err(e) => fill_error = Some(e),
        },
        Ordering::Greater => {}
        Ordering::Equal => fill_error = Some(""),
    }
    if shape_prefixes_match(&arr.shape, target) {
        return Ok(());
    }
    // Fill in missing dimensions
    match arr.rank().cmp(&target.len()) {
        Ordering::Less => match ctx.scalar_fill() {
            Ok(fill) => {
                let mut target_shape = arr.shape.clone();
                target_shape.insert(0, target_row_count);
                arr.fill_to_shape(&target_shape, fill);
                fill_error = None;
            }
            Err(e) => fill_error = Some(e),
        },
        Ordering::Greater => {}
        Ordering::Equal => {
            let target_shape = max_shape(arr.shape(), target);
            if arr.shape() != *target_shape {
                match ctx.scalar_fill() {
                    Ok(fill) => {
                        arr.fill_to_shape(&target_shape, fill);
                        fill_error = None;
                    }
                    Err(e) => fill_error = Some(e),
                }
            }
        }
    }
    if !shape_prefixes_match(&arr.shape, target) && fill_error.is_none() {
        fill_error = Some("");
    }
    if let Some(e) = fill_error {
        return Err(C::fill_error(ctx.error(format!(
            "Shapes {} and {} do not match{e}",
            arr.shape(),
            target,
        ))));
    }

    Ok(())
}

pub(crate) fn fill_value_shapes<C>(
    a: &mut Value,
    b: &mut Value,
    expand_fixed: bool,
    ctx: &C,
) -> Result<(), C::Error>
where
    C: FillContext,
{
    let a_err = fill_value_shape(a, b.shape(), expand_fixed, ctx).err();
    let b_err = fill_value_shape(b, a.shape(), expand_fixed, ctx).err();

    if shape_prefixes_match(a.shape(), b.shape())
        || !expand_fixed && (a.shape().starts_with(&[1]) || b.shape().starts_with(&[1]))
    {
        Ok(())
    } else if let Some(e) = a_err.or(b_err) {
        Err(e)
    } else {
        Err(C::fill_error(ctx.error(format!(
            "Shapes {} and {} do not match",
            a.shape(),
            b.shape(),
        ))))
    }
}

pub(crate) fn fill_array_shapes<A, B, C>(
    a: &mut Array<A>,
    b: &mut Array<B>,
    a_depth: usize,
    b_depth: usize,
    ctx: &C,
) -> Result<(), C::Error>
where
    A: ArrayValue,
    B: ArrayValue,
    C: FillContext,
{
    let a_depth = a_depth.min(a.rank());
    let b_depth = b_depth.min(b.rank());
    match (a_depth, b_depth) {
        (0, 0) => {
            let a_err = fill_array_shape(a, b.shape(), true, ctx).err();
            let b_err = fill_array_shape(b, a.shape(), true, ctx).err();
            if shape_prefixes_match(&a.shape, &b.shape) {
                Ok(())
            } else {
                Err(a_err.or(b_err).unwrap_or_else(|| {
                    C::fill_error(ctx.error(format!(
                        "Shapes {} and {} do not match",
                        a.shape(),
                        b.shape(),
                    )))
                }))
            }
        }
        (_, _) => {
            if a.row_count() != b.row_count() {
                return Err(C::fill_error(ctx.error(format!(
                    "Shapes {} and {} do not match",
                    a.shape(),
                    b.shape(),
                ))));
            }
            if !shape_prefixes_match(&a.shape[a_depth..], &b.shape[b_depth..]) {
                let mut new_a_rows = Vec::with_capacity(a.row_count());
                let mut new_b_rows = Vec::with_capacity(b.row_count());
                for (mut a_row, mut b_row) in a.rows().zip(b.rows()) {
                    fill_array_shapes(&mut a_row, &mut b_row, a_depth - 1, b_depth - 1, ctx)?;
                    new_a_rows.push(a_row);
                    new_b_rows.push(b_row);
                }
                *a = Array::from_row_arrays_infallible(new_a_rows);
                *b = Array::from_row_arrays_infallible(new_b_rows);
            }
            Ok(())
        }
    }
}

pub fn switch(
    count: usize,
    sig: Signature,
    copy_condition_under: bool,
    env: &mut Uiua,
) -> UiuaResult {
    // Get selector
    let selector = env.pop("switch index")?;
    let copied_selector = if copy_condition_under {
        Some(selector.clone())
    } else {
        None
    };
    let selector = selector.as_natural_array(env, "Switch index must be an array of naturals")?;
    if let Some(i) = selector.data.iter().find(|&&i| i >= count) {
        return Err(env.error(format!(
            "Switch index {i} is out of bounds for switch of size {count}"
        )));
    }
    // Switch
    if selector.rank() == 0 {
        // Scalar
        let i = selector.data[0];
        // Get function
        let Some(f) = env
            .rt
            .function_stack
            .drain(env.rt.function_stack.len() - count..)
            .nth(i)
        else {
            return Err(env.error(
                "Function stack was empty when getting switch function. \
                This is a bug in the interpreter.",
            ));
        };
        // Discard unused arguments
        let discard_start = env.rt.stack.len().saturating_sub(sig.args);
        if discard_start > env.rt.stack.len() {
            return Err(env.error("Stack was empty when discarding excess switch arguments."));
        }
        // `saturating_sub` and `max` handle incorrect explicit signatures
        let discard_end = (discard_start + sig.args + f.signature().outputs)
            .saturating_sub(f.signature().args + sig.outputs)
            .max(discard_start);
        if discard_end > env.rt.stack.len() {
            return Err(env.error("Stack was empty when discarding excess switch arguments."));
        }
        env.rt.stack.drain(discard_start..discard_end);
        env.call(f)?;
    } else {
        // Array
        // Collect arguments
        let mut args_rows: Vec<_> = Vec::with_capacity(sig.args);
        for i in 0..sig.args {
            let arg = env.pop(i + 1)?;
            if !arg.shape().starts_with(selector.shape()) {
                return Err(env.error(format!(
                    "The selector's shape {} is not compatible \
                    with the argument {}'s shape {}",
                    selector.shape(),
                    i + 1,
                    arg.shape(),
                )));
            }
            let row_shape = Shape::from(&arg.shape()[selector.rank()..]);
            args_rows.push(arg.into_row_shaped_slices(row_shape));
        }
        args_rows.reverse();
        // Collect functions
        let functions: Vec<(Function, usize)> = env
            .rt
            .function_stack
            .drain(env.rt.function_stack.len() - count..)
            .map(|f| {
                let args = if f.signature().outputs < sig.outputs {
                    f.signature().args + sig.outputs - f.signature().outputs
                } else {
                    f.signature().args
                };
                (f, args)
            })
            .collect();
        let mut outputs = multi_output(sig.outputs, Vec::new());
        // Switch with each selector element
        for elem in selector.data {
            let (f, args) = &functions[elem];
            for (i, arg) in args_rows.iter_mut().rev().enumerate().rev() {
                let arg = arg.next().unwrap();
                if i < *args {
                    env.push(arg);
                }
            }
            env.call(f.clone())?;
            for i in 0..sig.outputs {
                outputs[i].push(env.pop("switch output")?);
            }
        }
        // Collect output
        for output in outputs.into_iter().rev() {
            let mut new_value = Value::from_row_values(output, env)?;
            let mut new_shape = selector.shape.clone();
            new_shape.extend_from_slice(&new_value.shape()[1..]);
            *new_value.shape_mut() = new_shape;
            env.push(new_value);
        }
    }
    if let Some(selector) = copied_selector {
        env.push_temp(TempStack::Under, selector);
    }
    Ok(())
}

pub fn try_(env: &mut Uiua) -> UiuaResult {
    let f = env.pop_function()?;
    let handler = env.pop_function()?;
    let f_sig = f.signature();
    let handler_sig = handler.signature();
    if env.stack_height() < f_sig.args {
        for i in 0..f_sig.args {
            env.pop(i + 1)?;
        }
    }
    let backup = env.clone_stack_top(f_sig.args.min(handler_sig.args))?;
    if let Err(e) = env.call_clean_stack(f) {
        if handler_sig.args > f_sig.args {
            (env.rt.backend).save_error_color(e.message(), e.report().to_string());
            env.push(e.value());
        }
        for val in backup {
            env.push(val);
        }
        env.call(handler)?;
    }
    Ok(())
}

/// If a function fails on a byte array because no fill byte is defined,
/// convert the byte array to a number array and try again.
fn op_bytes_retry_fill<T, E: FillError>(
    bytes: Array<u8>,
    on_bytes: impl FnOnce(Array<u8>) -> Result<T, E>,
    on_nums: impl FnOnce(Array<f64>) -> Result<T, E>,
) -> Result<T, E> {
    match on_bytes(bytes.clone()) {
        Ok(res) => Ok(res),
        Err(err) if err.is_fill() => on_nums(bytes.convert()),
        Err(err) => Err(err),
    }
}

/// If a function fails on a byte array because no fill byte is defined,
/// convert the byte array to a number array and try again.
fn op_bytes_ref_retry_fill<T>(
    bytes: &Array<u8>,
    on_bytes: impl FnOnce(&Array<u8>) -> UiuaResult<T>,
    on_nums: impl FnOnce(&Array<f64>) -> UiuaResult<T>,
) -> UiuaResult<T> {
    match on_bytes(bytes) {
        Ok(res) => Ok(res),
        Err(err) if err.is_fill() => on_nums(&bytes.clone().convert()),
        Err(err) => Err(err),
    }
}

/// If a function fails on 2 byte arrays because no fill byte is defined,
/// convert the byte arrays to number arrays and try again.
fn op2_bytes_retry_fill<T, C: FillContext>(
    a: Array<u8>,
    b: Array<u8>,
    ctx: &C,
    on_bytes: impl FnOnce(Array<u8>, Array<u8>) -> Result<T, C::Error>,
    on_nums: impl FnOnce(Array<f64>, Array<f64>) -> Result<T, C::Error>,
) -> Result<T, C::Error> {
    if ctx.scalar_fill::<f64>().is_ok() {
        match on_bytes(a.clone(), b.clone()) {
            Ok(res) => Ok(res),
            Err(err) if C::is_fill_error(&err) => on_nums(a.convert(), b.convert()),
            Err(err) => Err(err),
        }
    } else {
        on_bytes(a, b)
    }
}

struct ArrayCmpSlice<'a, T>(&'a [T]);

impl<'a, T: ArrayValue> PartialEq for ArrayCmpSlice<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().zip(other.0).all(|(a, b)| a.array_eq(b))
    }
}

impl<'a, T: ArrayValue> Eq for ArrayCmpSlice<'a, T> {}

impl<'a, T: ArrayValue> PartialOrd for ArrayCmpSlice<'a, T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, T: ArrayValue> Ord for ArrayCmpSlice<'a, T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .iter()
            .zip(other.0)
            .map(|(a, b)| a.array_cmp(b))
            .find(|&o| o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}

impl<'a, T: ArrayValue> Hash for ArrayCmpSlice<'a, T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for elem in self.0 {
            elem.array_hash(state);
        }
    }
}

type FixedRows = Vec<
    Result<iter::Chain<Box<dyn ExactDoubleIterator<Item = Value>>, option::IntoIter<Value>>, Value>,
>;

struct FixedRowsData {
    rows: FixedRows,
    row_count: usize,
    is_empty: bool,
    all_scalar: bool,
    per_meta: PersistentMeta,
}

fn fixed_rows(
    prim: impl fmt::Display,
    outputs: usize,
    mut args: Vec<Value>,
    env: &Uiua,
) -> UiuaResult<FixedRowsData> {
    for a in 0..args.len() {
        let a_can_fill = args[a].length_is_fillable(env);
        for b in a + 1..args.len() {
            let b_can_fill = args[b].length_is_fillable(env);
            let mut err = None;
            if a_can_fill {
                let b_row_count = args[b].row_count();
                err = args[a].fill_length_to(b_row_count, env).err();
            }
            if err.is_none() && b_can_fill {
                let a_row_count = args[a].row_count();
                err = args[b].fill_length_to(a_row_count, env).err();
            }
            if err.is_none()
                && args[a].row_count() != args[b].row_count()
                && args[a].row_count() != 1
                && args[b].row_count() != 1
            {
                err = Some("");
            }
            if let Some(e) = err {
                return Err(env.error(format!(
                    "Cannot {prim} arrays with different number of rows, shapes {} and {}{e}",
                    args[a].shape(),
                    args[b].shape(),
                )));
            }
        }
    }
    let mut row_count = 0;
    let mut all_scalar = true;
    let mut all_1 = true;
    let is_empty = outputs > 0 && args.iter().any(|v| v.row_count() == 0);
    let mut per_meta = Vec::new();
    let fixed_rows: FixedRows = args
        .into_iter()
        .map(|mut v| {
            all_scalar = all_scalar && v.rank() == 0;
            if v.row_count() == 1 {
                v.undo_fix();
                Err(v)
            } else {
                let proxy = is_empty.then(|| v.proxy_row(env));
                row_count = row_count.max(v.row_count());
                all_1 = false;
                per_meta.push(v.take_per_meta());
                Ok(v.into_rows().chain(proxy))
            }
        })
        .collect();
    if all_1 {
        row_count = 1;
    }
    let per_meta = PersistentMeta::xor_all(per_meta);
    let row_count = row_count + is_empty as usize;
    Ok(FixedRowsData {
        rows: fixed_rows,
        row_count,
        is_empty,
        all_scalar,
        per_meta,
    })
}
