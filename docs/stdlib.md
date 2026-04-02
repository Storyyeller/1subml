# Standard Library Reference

The 1SubML standard library provides a small set of built-in modules for common tasks. All stdlib modules live under the `std` namespace.

To import a module:

```ml
import std.Vec;
import std.Map;
import std.Dyn;
import std.Exception;
```

You can also import specific members:

```ml
import std.Vec{empty; push; get};
import std.Exception{panic};
```

---

## Exception

Provides basic error handling via panics.

### Functions

**`panic: str -> never`**

Terminates execution with an error message. Since the return type is `never`, `panic` can be used in any expression context.

```ml
import std.Exception{panic};

let safe_div = fun (a, b) ->
    if b == 0 then panic "division by zero"
    else a / b;

print safe_div (10, 2); // 5
```

---

## Vec

A mutable, dynamically-sized array backed by JavaScript arrays.

### Type

`Vec.t[T]` (aliased from `vec[T]`) — a mutable vector holding elements of type `T`. The type parameter is invariant (`^`), meaning a `vec[int]` is not a subtype of `vec[any]`.

In function signatures, the variance annotations on `vec` control what operations are allowed:

- `vec[T<-never]` — read-only access (you can read `T` values out, but can't write)
- `vec[any<-T]` — write-only access (you can write `T` values in, but reads return `any`)
- `vec[T]` — full read/write access (shorthand for `vec[T<-T]`)

### Functions

**`empty: [T]. any -> vec[T]`**

Creates a new, empty vector.

```ml
import std.Vec;

let v: Vec.t[int] = Vec.empty ();
```

**`size: vec[any<-never] -> int`**

Returns the number of elements.

```ml
print Vec.size v; // 0
```

**`push: [T]. (vec[any<-T], T) -> any`**

Appends an element to the end.

```ml
Vec.push (v, 10);
Vec.push (v, 20);
print Vec.size v; // 2
```

**`pop: [T]. vec[T<-never] -> [\`Some T | \`None]`**

Removes and returns the last element, or `` `None`` if empty.

```ml
print Vec.pop v; // Some 20
```

**`get: [T]. (vec[T<-never], int) -> [\`Some T | \`None]`**

Returns the element at the given index, or `` `None`` if out of bounds.

```ml
print Vec.get (v, 0); // Some 10
print Vec.get (v, 99); // None
```

**`set: [T]. (vec[any<-T], int, T) -> any`**

Sets the element at the given index. Does nothing if out of bounds.

```ml
Vec.set (v, 0, 42);
```

**`clear: vec[any<-never] -> any`**

Removes all elements.

**`truncate: (vec[any<-never], int) -> any`**

Reduces the vector's length to at most the given size. Does nothing if the vector is already shorter.

**`map: [T; U]. (vec[T<-never], T -> U) -> vec[U]`**

Returns a new vector with the given function applied to each element.

```ml
import std.Vec;

let v = Vec.empty ();
Vec.push (v, 1);
Vec.push (v, 2);
Vec.push (v, 3);

let doubled = Vec.map (v, fun x -> x * 2);
print Vec.get (doubled, 1); // Some 4
```

**`filter: [T]. (vec[T<-never], T -> bool) -> vec[T]`**

Returns a new vector containing only elements for which the predicate returns `true`.

```ml
let evens = Vec.filter (v, fun x -> x % 2 == 0);
print Vec.size evens; // 1
```

---

## Map

A mutable hash map backed by JavaScript `Map`. Keys are compared by identity/equality (same semantics as `==` except that NaN is equal to itself).

### Type

`Map.t[K; V]` (aliased from `map[K; V]`) — a mutable map from keys of type `K` to values of type `V`. Both type parameters are invariant.

As with `Vec`, the variance annotations control access:

- `map[K<-never; V<-never]` — read-only
- `map[any<-K; any<-V]` — write-only

### Functions

**`empty: [K; V]. any -> map[K; V]`**

Creates a new, empty map.

```ml
import std.Map;

let m: Map.t[str; int] = Map.empty ();
```

**`set: [K; V]. (map[any<-K; any<-V], K, V) -> any`**

Inserts or updates a key-value pair.

```ml
Map.set (m, "x", 10);
Map.set (m, "y", 20);
```

**`get: [K; V]. (map[any<-K; V<-never], K) -> [\`Some V | \`None]`**

Returns the value for the given key, or `` `None`` if not found.

```ml
print Map.get (m, "x"); // Some 10
print Map.get (m, "z"); // None
```

**`has: [K]. (map[any<-K; any<-never], K) -> bool`**

Returns whether the map contains the given key.

```ml
print Map.has (m, "x"); // true
```

**`delete: [K]. (map[K; any<-never], K) -> any`**

Removes the entry for the given key.

```ml
Map.delete (m, "y");
```

**`size: map[any<-never; any<-never] -> int`**

Returns the number of entries.

```ml
print Map.size m; // 1
```

**`clear: map[any<-never; any<-never] -> any`**

Removes all entries.

**`keys: [K]. map[K<-never; any<-never] -> vec[K]`**

Returns a vector of all keys.

**`values: [V]. map[any<-never; V<-never] -> vec[V]`**

Returns a vector of all values.

**`items: [K; V]. map[K<-never; V<-never] -> vec[(K, V)]`**

Returns a vector of all key-value pairs as tuples.

```ml
import std.Map;
import std.Vec;

let m = Map.empty ();
Map.set (m, "a", 1);
Map.set (m, "b", 2);

let pairs = Map.items m;
print Vec.size pairs; // 2
```

---

## Dyn

Provides type-safe dynamic type tags. This serves a similar role to OCaml's extensible variants — it lets you create new "tags" at runtime that can wrap and unwrap values of a specific type, while all sharing a common `dyn` type.

This is useful when you need a heterogeneous collection or want to pass values of different types through a common interface without knowing all possible types upfront.

### Type

`Dyn.t` (aliased from `dyn`) — an opaque type that can hold a value of any type, tagged so it can be safely recovered.

### Functions

**`new: [T]. any -> {wrap: T -> dyn; unwrap: dyn -> [\`Some T | \`None]}`**

Creates a new unique tag for type `T`. Returns a record with two functions:

- `wrap` — wraps a value of type `T` into a `dyn`
- `unwrap` — attempts to unwrap a `dyn` back to type `T`. Returns `` `Some`` if the value was wrapped with this specific tag, `` `None`` otherwise.

Each call to `Dyn.new` creates a distinct tag. Values wrapped with one tag cannot be unwrapped with another, even if the types match.

```ml
import std.Dyn;

let int_tag = Dyn.new ();
let str_tag = Dyn.new ();

let a: dyn = int_tag.wrap 42;
let b: dyn = str_tag.wrap "hello";

print int_tag.unwrap a; // Some 42
print int_tag.unwrap b; // None
print str_tag.unwrap b; // Some "hello"
```

### Example: heterogeneous collection

```ml
import std.Dyn;
import std.Vec;

let int_tag = Dyn.new ();
let str_tag = Dyn.new ();

let v: Vec.t[dyn] = Vec.empty ();
Vec.push (v, int_tag.wrap 1);
Vec.push (v, str_tag.wrap "two");
Vec.push (v, int_tag.wrap 3);

// Later, recover the values by trying each tag
let describe = fun d ->
    match int_tag.unwrap d with
    | `Some n -> "int"
    | `None -> match str_tag.unwrap d with
        | `Some s -> "str"
        | `None -> "unknown"
    ;
```
