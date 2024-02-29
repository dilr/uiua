//! Algorithms for looping modifiers

use crate::{
    array::{Array, ArrayValue},
    value::Value,
    Boxed, Primitive, Signature, Uiua, UiuaResult,
};

use super::multi_output;

pub fn flip<A, B, C>(f: impl Fn(A, B) -> C + Copy) -> impl Fn(B, A) -> C + Copy {
    move |b, a| f(a, b)
}

pub fn repeat(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let n = env
        .pop("repetition count")?
        .as_num(env, "Repetitions must be a natural number or infinity")?;

    if n.is_infinite() {
        let sig = f.signature();
        if sig.args == 0 {
            return Err(env.error(format!(
                "Converging {}'s function must have at least 1 argument",
                Primitive::Repeat.format()
            )));
        }
        if sig.args != sig.outputs {
            return Err(env.error(format!(
                "Converging {}'s function must have a net stack change of 0, \
                but its signature is {sig}",
                Primitive::Repeat.format()
            )));
        }
        let mut prev = env.pop(1)?;
        env.push(prev.clone());
        loop {
            env.call(f.clone())?;
            let next = env.pop("converging function result")?;
            let converged = next == prev;
            if converged {
                env.push(next);
                break;
            } else {
                env.push(next.clone());
                prev = next;
            }
        }
    } else {
        if n < 0.0 || n.fract() != 0.0 {
            return Err(env.error("Repetitions must be a natural number or infinity"));
        }
        let n = n as usize;
        for _ in 0..n {
            env.call(f.clone())?;
        }
    }
    Ok(())
}

pub fn do_(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let g = env.pop_function()?;
    let f_sig = f.signature();
    let g_sig = g.signature();
    if g_sig.outputs < 1 {
        return Err(env.error(format!(
            "Do's condition function must return at least 1 value, \
            but its signature is {g_sig}"
        )));
    }
    let copy_count = g_sig.args.saturating_sub(g_sig.outputs - 1);
    let g_sub_sig = Signature::new(g_sig.args, g_sig.outputs + copy_count - 1);
    let comp_sig = f_sig.compose(g_sub_sig);
    if comp_sig.args != comp_sig.outputs {
        return Err(env.error(format!(
            "Do's functions must have a net stack change of 0, \
            but the composed signature of {f_sig} and {g_sig}, \
            minus the condition, is {comp_sig}"
        )));
    }
    loop {
        for _ in 0..copy_count {
            env.push(env.stack()[env.stack().len() - copy_count].clone());
        }
        env.call(g.clone())?;
        let cond = env
            .pop("do condition")?
            .as_bool(env, "Do condition must be a boolean")?;
        if !cond {
            break;
        }
        env.call(f.clone())?;
    }
    Ok(())
}

pub fn partition(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    collapse_groups(
        Primitive::Partition,
        Value::partition_groups,
        "⊜ partition indices array must be a list of integers",
        "⊜ partition's function has signature |2.1, so it is the reducing form. \
        Its indices array must be a list of integers",
        env,
    )
}

pub fn unpartition_part1(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let sig = f.signature();
    if sig != (1, 1) {
        return Err(env.error(format!(
            "Cannot undo {} on function with signature {sig}",
            Primitive::Partition.format()
        )));
    }
    let partitioned = env.pop(1)?;
    // Untransform rows
    let mut untransformed = Vec::with_capacity(partitioned.row_count());
    for row in partitioned.into_rows().rev() {
        env.push(row);
        env.call(f.clone())?;
        untransformed.push(Boxed(env.pop("unpartitioned row")?));
    }
    untransformed.reverse();
    env.push(Array::from_iter(untransformed));
    Ok(())
}

pub fn unpartition_part2(env: &mut Uiua) -> UiuaResult {
    let untransformed = env.pop(1)?;
    let markers = env
        .pop(2)?
        .as_ints(env, "⊜ partition markers must be a list of integers")?;
    let original = env.pop(3)?;
    // Count partition markers
    let mut marker_partitions: Vec<(isize, usize)> = Vec::new();
    let mut markers = markers.into_iter();
    if let Some(mut prev) = markers.next() {
        marker_partitions.push((prev, 1));
        for marker in markers {
            if marker == prev {
                marker_partitions.last_mut().unwrap().1 += 1;
            } else {
                marker_partitions.push((marker, 1));
            }
            prev = marker;
        }
    }
    let positive_partitions = marker_partitions.iter().filter(|(m, _)| *m > 0).count();
    if positive_partitions != untransformed.row_count() {
        return Err(env.error(format!(
            "Cannot undo {} because the partitioned array \
            originally had {} rows, but now it has {}",
            Primitive::Partition.format(),
            positive_partitions,
            untransformed.row_count()
        )));
    }

    // Unpartition
    let mut untransformed_rows = untransformed.into_rows().map(Value::unboxed);
    let mut unpartitioned = Vec::with_capacity(marker_partitions.len() * original.row_len());
    let mut original_offset = 0;
    for (marker, part_len) in marker_partitions {
        if marker > 0 {
            unpartitioned.extend(untransformed_rows.next().unwrap().into_rows());
        } else {
            unpartitioned
                .extend((original_offset..original_offset + part_len).map(|i| original.row(i)));
        }
        original_offset += part_len;
    }
    env.push(Value::from_row_values(unpartitioned, env)?);
    Ok(())
}

pub fn ungroup_part1(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let sig = f.signature();
    if sig != (1, 1) {
        return Err(env.error(format!(
            "Cannot undo {} on function with signature {sig}",
            Primitive::Group.format()
        )));
    }
    let grouped = env.pop(1)?;

    // Untransform rows
    let mut ungrouped_rows = Vec::with_capacity(grouped.row_count());
    for mut row in grouped.into_rows().rev() {
        env.push(row);
        env.call(f.clone())?;
        row = env.pop("ungrouped row")?;
        ungrouped_rows.push(Boxed(row));
    }
    ungrouped_rows.reverse();
    env.push(Array::from_iter(ungrouped_rows));
    Ok(())
}

pub fn ungroup_part2(env: &mut Uiua) -> UiuaResult {
    let ungrouped_rows = env.pop(1)?;
    let indices = env
        .pop(2)?
        .as_integer_array(env, "⊕ group indices must be an array of integers")?;
    let original = env.pop(3)?;

    if (indices.data.iter())
        .any(|&index| index >= 0 && index as usize >= ungrouped_rows.row_count())
    {
        return Err(env.error(format!(
            "Cannot undo {} because the grouped array's \
            length changed from {} to {}",
            Primitive::Group.format(),
            indices.element_count(),
            ungrouped_rows.row_count(),
        )));
    }

    // Ungroup
    let mut ungrouped_rows: Vec<_> = ungrouped_rows
        .into_rows()
        .map(|row| row.unboxed().into_rows())
        .collect();
    let mut ungrouped = Vec::with_capacity(indices.element_count() * original.row_len());
    for (i, &index) in indices.data.iter().enumerate() {
        if index >= 0 {
            ungrouped.push(ungrouped_rows[index as usize].next().ok_or_else(|| {
                env.error("A group's length was modified between grouping and ungrouping")
            })?);
        } else {
            ungrouped.push(original.row(i));
        }
    }
    let mut val = Value::from_row_values(ungrouped, env)?;
    val.shape_mut().remove(0);
    for &dim in indices.shape().iter().rev() {
        val.shape_mut().insert(0, dim);
    }
    env.push(val);
    Ok(())
}

impl Value {
    fn partition_groups(self, markers: Array<isize>, env: &Uiua) -> UiuaResult<Vec<Self>> {
        if markers.rank() != 1 {
            return Err(env.error(format!(
                "{} markers must be a list of integers, \
                but it is rank {}",
                Primitive::Partition.format(),
                markers.rank(),
            )));
        }
        let markers = &markers.data;
        Ok(match self {
            Value::Num(arr) => arr
                .partition_groups(markers, env)?
                .map(Into::into)
                .collect(),
            #[cfg(feature = "bytes")]
            Value::Byte(arr) => arr
                .partition_groups(markers, env)?
                .map(Into::into)
                .collect(),
            Value::Complex(arr) => arr
                .partition_groups(markers, env)?
                .map(Into::into)
                .collect(),
            Value::Char(arr) => arr
                .partition_groups(markers, env)?
                .map(Into::into)
                .collect(),
            Value::Box(arr) => arr
                .partition_groups(markers, env)?
                .map(Into::into)
                .collect(),
        })
    }
}

impl<T: ArrayValue> Array<T> {
    fn partition_groups(
        self,
        markers: &[isize],
        env: &Uiua,
    ) -> UiuaResult<impl Iterator<Item = Self>> {
        if markers.len() != self.row_count() {
            return Err(env.error(format!(
                "Cannot partition array of shape {} with markers of length {}",
                self.shape(),
                markers.len()
            )));
        }
        let mut groups = Vec::new();
        let mut last_marker = isize::MAX;
        for (row, &marker) in self.into_rows().zip(markers) {
            if marker > 0 {
                if marker != last_marker {
                    groups.push(Vec::new());
                }
                groups.last_mut().unwrap().push(row);
            }
            last_marker = marker;
        }
        Ok(groups.into_iter().map(Array::from_row_arrays_infallible))
    }
}

pub fn group(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    collapse_groups(
        Primitive::Group,
        Value::group_groups,
        "⊕ group indices array must be an array of integers",
        "⊕ group's function has signature |2.1, so it is the reducing form. \
        Its indices array must be a list of integers",
        env,
    )
}

impl Value {
    fn group_groups(self, indices: Array<isize>, env: &Uiua) -> UiuaResult<Vec<Self>> {
        Ok(match self {
            Value::Num(arr) => arr.group_groups(indices, env)?.map(Into::into).collect(),
            #[cfg(feature = "bytes")]
            Value::Byte(arr) => arr.group_groups(indices, env)?.map(Into::into).collect(),
            Value::Complex(arr) => arr.group_groups(indices, env)?.map(Into::into).collect(),
            Value::Char(arr) => arr.group_groups(indices, env)?.map(Into::into).collect(),
            Value::Box(arr) => arr.group_groups(indices, env)?.map(Into::into).collect(),
        })
    }
}

impl<T: ArrayValue> Array<T> {
    fn group_groups(
        self,
        indices: Array<isize>,
        env: &Uiua,
    ) -> UiuaResult<impl Iterator<Item = Self>> {
        if !self.shape().starts_with(indices.shape()) {
            return Err(env.error(format!(
                "Cannot {} array of shape {} with indices of shape {}",
                Primitive::Group.format(),
                self.shape(),
                indices.shape()
            )));
        }
        let Some(&max_index) = indices.data.iter().max() else {
            return Ok(Vec::<Vec<Self>>::new()
                .into_iter()
                .map(Array::from_row_arrays_infallible));
        };
        let mut groups: Vec<Vec<Self>> = vec![Vec::new(); max_index.max(0) as usize + 1];
        let row_shape = self.shape()[indices.rank()..].into();
        for (g, r) in (indices.data.into_iter()).zip(self.into_row_shaped_slices(row_shape)) {
            if g >= 0 {
                groups[g as usize].push(r);
            }
        }
        Ok(groups.into_iter().map(Array::from_row_arrays_infallible))
    }
}

fn collapse_groups(
    prim: Primitive,
    get_groups: impl Fn(Value, Array<isize>, &Uiua) -> UiuaResult<Vec<Value>>,
    agg_indices_error: &'static str,
    red_indices_error: &'static str,
    env: &mut Uiua,
) -> UiuaResult {
    let f = env.pop_function()?;
    let sig = f.signature();
    match (sig.args, sig.outputs) {
        (0 | 1, outputs) => {
            let indices = env.pop(1)?.as_integer_array(env, agg_indices_error)?;
            let values = env.pop(2)?;
            let groups = get_groups(values, indices, env)?;
            let mut rows = multi_output(outputs, Vec::with_capacity(groups.len()));
            env.without_fill(|env| -> UiuaResult {
                for group in groups {
                    env.push(group);
                    env.call(f.clone())?;
                    for i in 0..outputs.max(1) {
                        let value = env.pop(|| format!("{}'s function result", prim.format()))?;
                        if sig.args == 1 {
                            rows[i].push(value);
                        }
                    }
                }
                Ok(())
            })?;
            for rows in rows.into_iter().rev() {
                env.push(Value::from_row_values(rows, env)?);
            }
        }
        (2, 1) => {
            let indices = env.pop(1)?.as_integer_array(env, red_indices_error)?;
            let values = env.pop(2)?;
            let mut groups = get_groups(values, indices, env)?.into_iter();
            let mut acc = match env.value_fill().cloned() {
                Some(acc) => acc,
                None => groups.next().ok_or_else(|| {
                    env.error(format!(
                        "Cannot do aggregating {} with no groups",
                        prim.format()
                    ))
                })?,
            };
            env.without_fill(|env| -> UiuaResult {
                for row in groups {
                    env.push(row);
                    env.push(acc);
                    env.call(f.clone())?;
                    acc = env.pop("reduced function result")?;
                }
                env.push(acc);
                Ok(())
            })?;
        }
        _ => {
            return Err(env.error(format!(
                "Cannot {} with a function with signature {sig}",
                prim.format()
            )))
        }
    }
    Ok(())
}
