// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::StringId;

pub fn tuple_name(strings: &mut lasso::Rodeo, n: u32) -> StringId {
    strings.get_or_intern(format!("_{}", n))
}

pub fn get_tuple_name(strings: &lasso::Rodeo, n: u32) -> Option<StringId> {
    strings.get(format!("_{}", n))
}

pub fn is_tuple_name(strings: &lasso::Rodeo, s: StringId) -> Option<u32> {
    strings.resolve(&s).strip_prefix('_')?.parse().ok()
}

pub fn is_partial_tuple_record(strings: &lasso::Rodeo, fields: impl Iterator<Item = StringId>) -> Option<u32> {
    let mut max = 0;
    let mut count = 0;
    for f in fields {
        // Early return by ? if not a tuple field
        max = max.max(is_tuple_name(strings, f)? + 1);
        count += 1;
    }

    if max <= 1 {
        return None; // No tuple shorthand sytanx for 0 or 1 element tuples
    }

    // Only use tuple syntax if density >= 1/3,
    // to avoid bloated output if the user manually writes e.g. {_999}.
    if max <= count * 3 { Some(max) } else { None }
}
