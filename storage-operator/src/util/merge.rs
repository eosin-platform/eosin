#![allow(dead_code)]

use serde_json::Value;

/// Deep merge two json values. Moves the values of `b` into `a`.
/// Source: <https://stackoverflow.com/a/54118457>
pub fn deep_merge(a: &mut Value, b: Value) {
    match (a, b) {
        // Both values are objects. Merge them and only unset
        // fields when null is explicitly specified.
        (&mut Value::Object(ref mut a), Value::Object(b)) => b.into_iter().for_each(|(k, v)| {
            if v.is_null() {
                a.remove(&k);
            } else {
                deep_merge(a.entry(k).or_insert(Value::Null), v);
            }
        }),
        // One or both or the values are not capable of deep merge.
        (a, b) => {
            *a = b;
        }
    }
}
