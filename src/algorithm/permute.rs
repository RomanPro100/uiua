use ecow::EcoVec;

use crate::{
    types::push_empty_rows_value, val_as_arr, Array, ArrayValue, Function, Primitive, Uiua,
    UiuaResult, Value,
};

use super::{monadic::range, table::table_impl, validate_size};

pub fn tuples(env: &mut Uiua) -> UiuaResult {
    let f = env.pop_function()?;
    let sig = f.signature();
    if sig.outputs != 1 {
        return Err(env.error(format!(
            "{}'s function must have 1 output, \
            but its signature is {sig}",
            Primitive::Tuples.format()
        )));
    }
    match sig.args {
        1 => tuple1(f, env)?,
        2 => tuple2(f, env)?,
        _ => {
            return Err(env.error(format!(
                "{}'s function must have 1 or 2 arguments, \
                but its signature is {sig}",
                Primitive::Tuples.format()
            )))
        }
    }
    Ok(())
}

fn tuple1(f: Function, env: &mut Uiua) -> UiuaResult {
    let mut xs = env.pop(1)?;
    let mut results = Vec::new();
    let mut per_meta = xs.take_per_meta();
    if xs.row_count() == 0 {
        if !push_empty_rows_value(&f, [&xs], false, &mut per_meta, env) {
            let mut proxy = xs.proxy_row(env);
            proxy.fix();
            env.push(proxy);
            _ = env.call_maintain_sig(f);
            results.push(env.pop("rows' function result")?);
        }
    } else {
        env.without_fill(|env| -> UiuaResult {
            for n in 0..=xs.row_count() {
                env.push(xs.slice_rows(0, n));
                env.call(f.clone())?;
                results.push(env.pop("tuples's function result")?);
            }
            Ok(())
        })?;
    }
    let mut val = Value::from_row_values(results, env)?;
    if xs.row_count() == 0 {
        val.pop_row();
    }
    env.push(val);
    Ok(())
}

fn tuple2(f: Function, env: &mut Uiua) -> UiuaResult {
    let k = env.pop(1)?;
    let mut xs = env.pop(2)?;
    'blk: {
        if let Some(prim) = f.as_primitive(&env.asm) {
            let res = match prim {
                Primitive::Lt => k.choose(&xs, false, false, env)?,
                Primitive::Le => k.choose(&xs, false, true, env)?,
                Primitive::Gt => k.choose(&xs, true, false, env)?,
                Primitive::Ge => k.choose(&xs, true, true, env)?,
                Primitive::Ne => k.permute(xs, env)?,
                _ => break 'blk,
            };
            env.push(res);
            return Ok(());
        }
    }
    if xs.rank() == 0 {
        return Err(env.error("Cannot get tuples of scalar"));
    }
    let k = k.as_nat(env, "Tuple size must be a natural number")?;
    match k {
        0 => {
            xs = xs.first_dim_zero();
            xs.shape_mut()[0] = 1;
            xs.shape_mut().insert(1, 0);
            xs.validate_shape();
        }
        1 => xs.shape_mut().insert(1, 1),
        2 => {
            let range: Value = match range(&[xs.row_count() as isize], env)? {
                Ok(data) => Array::new(xs.row_count(), data).into(),
                Err(data) => Array::new(xs.row_count(), data).into(),
            };
            env.push(range.clone());
            env.push(range);
            table_impl(f, env)?;
            let mut table = env.pop("table's function result")?;
            if table.rank() > 2 {
                return Err(env.error(format!(
                    "{}'s function must return a scalar, \
                    but the result is rank-{}",
                    Primitive::Tuples.format(),
                    table.rank() - 2
                )));
            }
            table.transpose();
            let table = table.as_natural_array(
                env,
                "tuples's function must return \
                        an array of naturals",
            )?;
            let mut rows = Vec::new();
            for (i, counts) in table.row_slices().enumerate() {
                for (j, &count) in counts.iter().enumerate() {
                    for _ in 0..count {
                        rows.push(xs.row(i));
                        rows.push(xs.row(j));
                    }
                }
            }
            xs = Value::from_row_values(rows, env)?;
            xs.shape_mut()[0] /= 2;
            xs.shape_mut().insert(1, 2);
            xs.validate_shape();
        }
        k => {
            fn inner<T: Clone>(
                arr: &Array<T>,
                k: usize,
                f: Function,
                env: &mut Uiua,
            ) -> UiuaResult<Array<T>> {
                let mut curr = vec![0; k];
                let mut data = EcoVec::new();
                let mut count = 0;
                let row_count = arr.row_count();
                let row_len = arr.row_len();
                'outer: loop {
                    // println!("curr: {curr:?}");
                    let mut add_it = true;
                    'ij: for (ii, &i) in curr.iter().enumerate() {
                        for &j in curr.iter().skip(ii + 1) {
                            // println!("i: {i}, j: {j}");
                            env.push(i);
                            env.push(j);
                            env.call(f.clone())?;
                            add_it &= env
                                .pop("tuples's function result")?
                                .as_bool(env, "tuples of 3 or more must return a boolean")?;
                            if !add_it {
                                break 'ij;
                            }
                        }
                    }
                    if add_it {
                        for &i in &curr {
                            data.extend_from_slice(&arr.data[i * row_len..][..row_len]);
                        }
                        count += 1;
                    }
                    // Increment curr
                    for i in (0..k).rev() {
                        if curr[i] == row_count - 1 {
                            curr[i] = 0;
                        } else {
                            curr[i] += 1;
                            continue 'outer;
                        }
                    }
                    break;
                }
                let mut shape = arr.shape.clone();
                shape[0] = count;
                shape.insert(1, k);
                Ok(Array::new(shape, data))
            }
            xs = match &xs {
                Value::Num(a) => inner(a, k, f, env)?.into(),
                Value::Byte(a) => inner(a, k, f, env)?.into(),
                Value::Complex(a) => inner(a, k, f, env)?.into(),
                Value::Char(a) => inner(a, k, f, env)?.into(),
                Value::Box(a) => inner(a, k, f, env)?.into(),
            };
        }
    }
    env.push(xs);
    Ok(())
}

impl Value {
    /// `choose` all combinations of `k` rows from a value
    pub fn choose(&self, from: &Self, reverse: bool, same: bool, env: &Uiua) -> UiuaResult<Self> {
        let k = self.as_nat(env, "Choose k must be an integer")?;
        if let Ok(n) = from.as_nat(env, "") {
            return combinations(n, k, same, env).map(Into::into);
        }
        val_as_arr!(from, |a| a.choose(k, reverse, same, env).map(Into::into))
    }
    /// `permute` all combinations of `k` rows from a value
    pub fn permute(&self, from: Self, env: &Uiua) -> UiuaResult<Self> {
        let k = self.as_nat(env, "Permute k must be an integer")?;
        if let Ok(n) = from.as_nat(env, "") {
            return permutations(n, k, env).map(Into::into);
        }
        val_as_arr!(from, |a| a.permute(k, env).map(Into::into))
    }
}

fn combinations(n: usize, k: usize, same: bool, env: &Uiua) -> UiuaResult<f64> {
    if k > n {
        return Err(env.error(format!(
            "Cannot choose combinations of {k} rows \
            from array of shape {}",
            n
        )));
    }
    let calc_n = if same { n + k - 1 } else { n };
    Ok((1..=k.min(calc_n - k))
        .map(|i| (calc_n + 1 - i) as f64 / i as f64)
        .product())
}

fn permutations(n: usize, k: usize, env: &Uiua) -> UiuaResult<f64> {
    if k > n {
        return Err(env.error(format!(
            "Cannot get permutations of {k} rows \
            from array of shape {}",
            n
        )));
    }
    Ok((1..=n).rev().take(k).map(|i| i as f64).product())
}

impl<T: ArrayValue> Array<T> {
    /// `choose` all combinations of `k` rows from this array
    fn choose(&self, k: usize, rev: bool, same: bool, env: &Uiua) -> UiuaResult<Self> {
        if self.rank() == 0 {
            return Err(env.error("Cannot choose from scalar"));
        }
        let n = self.row_count();
        let mut shape = self.shape.clone();
        let combinations = combinations(n, k, same, env)?;
        if combinations.is_nan() {
            return Err(env.error("Combinatorial explosion"));
        }
        if combinations > usize::MAX as f64 {
            return Err(env.error(format!("{combinations} combinations would be too many")));
        }
        shape[0] = combinations.round() as usize;
        shape.insert(1, k);
        let elem_count = validate_size::<T>(shape.iter().copied(), env)?;
        let row_len = self.row_len();
        let at = |i| &self.data[i * row_len..][..row_len];
        Ok(match (k, n - k) {
            (1, _) => Array::new(shape, self.data.clone()),
            (_, 0) if !same => Array::new(shape, self.data.clone()),
            (_, 1) if !same => {
                let mut data = EcoVec::with_capacity(elem_count);
                if rev {
                    for i in (0..n).rev() {
                        for (j, row) in self.row_slices().enumerate().rev() {
                            if i != j {
                                data.extend_from_slice(row);
                            }
                        }
                    }
                } else {
                    for i in (0..n).rev() {
                        for (j, row) in self.row_slices().enumerate() {
                            if i != j {
                                data.extend_from_slice(row);
                            }
                        }
                    }
                }
                Array::new(shape, data)
            }
            (2, _) => {
                let mut data = EcoVec::with_capacity(elem_count);
                let mut add = |i, j| {
                    data.extend_from_slice(at(i));
                    data.extend_from_slice(at(j));
                };
                match (rev, same) {
                    (false, false) => (0..n - 1).for_each(|i| (i + 1..n).for_each(|j| add(i, j))),
                    (true, false) => (1..n).for_each(|i| (0..i).for_each(|j| add(i, j))),
                    (false, true) => (0..n).for_each(|i| (i..n).for_each(|j| add(i, j))),
                    (true, true) => (0..n).for_each(|i| (0..=i).for_each(|j| add(i, j))),
                }
                Array::new(shape, data)
            }
            (3, _) => {
                let mut data = EcoVec::with_capacity(elem_count);
                let mut add = |i, j, k| {
                    data.extend_from_slice(at(i));
                    data.extend_from_slice(at(j));
                    data.extend_from_slice(at(k));
                };
                match (rev, same) {
                    (false, false) => (0..n - 2).for_each(|i| {
                        (i + 1..n - 1).for_each(|j| (j + 1..n).for_each(|k| add(i, j, k)))
                    }),
                    (true, false) => {
                        (2..n).for_each(|i| (1..i).for_each(|j| (0..j).for_each(|k| add(i, j, k))))
                    }
                    (false, true) => {
                        (0..n).for_each(|i| (i..n).for_each(|j| (j..n).for_each(|k| add(i, j, k))))
                    }
                    (true, true) => (0..n)
                        .for_each(|i| (0..=i).for_each(|j| (0..=j).for_each(|k| add(i, j, k)))),
                }
                Array::new(shape, data)
            }
            _ => {
                let mut data = EcoVec::with_capacity(elem_count);
                let mut indices = vec![0; k];
                let op = match (rev, same) {
                    (false, false) => PartialOrd::lt,
                    (true, false) => PartialOrd::gt,
                    (false, true) => PartialOrd::le,
                    (true, true) => PartialOrd::ge,
                };
                'outer: loop {
                    env.respect_execution_limit()?;
                    if (indices.iter().skip(1))
                        .zip(&indices)
                        .all(|(a, b)| op(a, b))
                    {
                        for &i in indices.iter().rev() {
                            data.extend_from_slice(at(i));
                        }
                    }
                    // Increment indices
                    for i in 0..k {
                        indices[i] += 1;
                        if indices[i] == n {
                            indices[i] = 0;
                        } else {
                            continue 'outer;
                        }
                    }
                    break;
                }
                Array::new(shape, data)
            }
        })
    }
    /// `permute` all combinations of `k` rows from this array
    fn permute(self, k: usize, env: &Uiua) -> UiuaResult<Self> {
        if self.rank() == 0 {
            return Err(env.error("Cannot permute scalar"));
        }
        let n = self.row_count();
        let mut shape = self.shape.clone();
        let permutations = permutations(n, k, env)?;
        if permutations.is_nan() {
            return Err(env.error("Combinatorial explosion"));
        }
        if permutations > usize::MAX as f64 {
            return Err(env.error(format!("{permutations} permutations would be too many")));
        }
        shape[0] = permutations.round() as usize;
        shape.insert(1, k);
        let elem_count = validate_size::<T>(shape.iter().copied(), env)?;
        let mut data = EcoVec::with_capacity(elem_count);
        let row_len = self.row_len();
        if k == n {
            let mut stack = Vec::new();
            stack.push((0, EcoVec::from(self.data)));
            while let Some((start, curr)) = stack.pop() {
                if start == n {
                    unsafe { data.extend_from_trusted(curr) };
                } else {
                    for i in (start..n).rev() {
                        let mut next = curr.clone();
                        next.make_mut().swap(start, i);
                        stack.push((start + 1, next));
                    }
                }
            }
        } else {
            let mut indices: Vec<usize> = (0..k).collect();
            let mut counts = vec![0; k];
            let mut curr = indices.clone();
            'outer: loop {
                curr.clone_from(&indices);
                counts.iter_mut().for_each(|c| *c = 0);
                for &idx in &curr {
                    data.extend_from_slice(&self.data[idx * row_len..][..row_len]);
                }
                let mut i = 0;
                while i < k {
                    if counts[i] < i {
                        let a = if i % 2 == 0 { 0 } else { counts[i] };
                        curr.swap(a, i);
                        for &idx in &curr {
                            data.extend_from_slice(&self.data[idx * row_len..][..row_len]);
                        }
                        counts[i] += 1;
                        i = 0;
                    } else {
                        counts[i] = 0;
                        i += 1;
                    }
                }

                // Next indices
                i = k;
                while i > 0 {
                    i -= 1;
                    if indices[i] < n - k + i {
                        indices[i] += 1;
                        for j in i + 1..k {
                            indices[j] = indices[j - 1] + 1;
                        }
                        continue 'outer;
                    }
                }
                break;
            }
        }
        Ok(Array::new(shape, data))
    }
}
