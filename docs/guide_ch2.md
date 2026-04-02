# Chapter 2: Types

[Back to Table of Contents](guide.md) | Previous: [Chapter 1: Expressions and values](guide_ch1.md) | Next: [Chapter 3: Polymorphism and modules](guide_ch3.md)

---

1SubML's powerful type inference means that no type annotations are required except in a few specific cases. However, even when type annotations are not necessary, it can be helpful to add type annotations in order to clarify intent in the code and help narrow down the scope of type errors if there is a type error. 

## Where can I put type annotations?

First off, expressions can manually be annotated with a type via `(expr: type)`, e.g. `(24: int)` or `(fun x -> x: str -> str)`.

Second, type annotations can be added to any variable pattern, e.g. `let a: int = 43;`. This applies to anywhere that variable patterns can appear, including `let` bindings, `match` cases, function arguments, and nested within other patterns. 

```ocaml
let (a: int, b: str, c, d: float) = (1, "", "", 3.2);
let {a; b: int; c=x: int; d={e: float}} = {a=1; b=2; c=3; d={e=4.4}};

match `Foo {x=32} with
| `Foo {x: int} -> x;

let add = fun {a: int; b: int} -> a + b;
```

You can even add type annotations to `_` variable patterns, e.g. (`let _: int = 42;`). `_` patterns don't bind a variable, but the type constraint will still be applied to the matched value.

Third, you can add type annotations to fields in a record literal:

```ocaml
let a = 42;
let b = -9.7;

let r = {
  a: int;
  x: int = a;
  mut b: float;
  mut y: float = b
};
```

Lastly, you can also annotate the return type of a function definition. Return type annotations use `::` instead of `:`:

```ocaml
let add = fun {a: int; b: int} :: int -> a + b;
```

When the return type annotation is itself a function type (e.g. `int -> int`), it has to be surrounded in parenthesis to avoid ambiguity. For example, here's how to correctly annotate all the arguments and return types of a curried function:


```ocaml
let add_curried = fun a: int :: (int -> int) -> 
    fun b: int :: int -> a + b;

print (add_curried 4) 22; // 26
```

## Structural types

* Basic types: `bool`, `float`, `int`, `str`, `any`, and `never`

`any` is a special type that can hold *any* value. `never` is a special type that can *never* hold a value, and thus it is impossible to create a value of type `never`. Any code with a value of type `never` is thus guaranteed to be unreachable.

Note that this differs from the "any" type of some other languages, where "any" is used as a deliberately unsound escape hatch from the type system. In 1SubML, `any` is 100% sound and type safe. It can hold any value, but you can't actually do anything with values of `any` type (other than equality comparisons), so it is still type safe.

* Function types: `int -> int`

`a -> b -> c` is parsed as `a -> (b -> c)`. If the argument type is itself a function type, you need to wrap it in parentheses, e.g. `(int -> int) -> int`.


* Record types: `{field1: int; field2: str; mut field3: float; mut field4: float <- never}`

Mutable fields have two associated types, the type that can be *read* from the field, and the type that can be *written* to the field. If you specify a single type (e.g. `mut a: float`), it will be used as both the read and write type. 

In rare cases, you may want to specify separate read and write types, which can be done via `mut field: read_ty <- write_ty`. For example, in the type `{mut a: any <- float}`, only floats can be written to field `a`, but when the field is accessed, it will be read as type `any`.


You can also use tuple syntax for record types. For example, `(int, str, float)` is shorthand syntax for the record type `{_0: int; _1: str; _2: float}`.


* Variant types: ``[`Foo int | `Bar float | `Baz]``

In 1SubML, variants always have an associated value. However, if you don't care about it, you can omit that part from the type annotation, in which case it is treated as `any`. For example, ``[`Some int | `None]`` is shorthand for the type ``[`Some int | `None any]``.

> Note: The list of cases cannot be empty. `[]` is not a valid type. Use `never` instead.

* Inferred types: `_`

Each `_` in the source code creates a fresh inference variable whose type will be inferred by the compiler. It is useful if you only want to specify part of a type, e.g. `{a: int; b: _}`

* Recursive types: ``rec list = [`Some (int, list) | `None]``

The general form of recursive types is `rec name = type`, where `name` can appear within `type`. In order to ensure that the recursive type is well-formed, `type` must be a record, function, variant, or recursive type.

Recursive types written this way are *equirecursive*, meaning that each type is *equal* to its infinite unrolling, with no separate wrapping or unwrapping steps required. This means that there can be multiple different representations of the same underlying type. For example, the above type could alternatively be written ```rec list=[`Nil | `Cons (int, [`Nil | `Cons (int, list)])]``` or even ```[`Nil | `Cons (int, rec list=[`Nil | `Cons (int, list)])]```. All of these result in the same infinite expansion, and thus are considered alternate representations of the same type.

In addition to the *structural* types described above, 1SubML also has *named* and *polymorphic* types, which will be covered in [newtype definitions](#newtype-definitions) and [Chapter 3](guide_ch3.md).

## Type aliases

To avoid repeatedly writing out a long type, you can also define an *alias* of a type via `alias <name> = <type>;`:

```ml
alias foo = int -> int;

// Equivalent to let f: int -> int = ...
let f: foo = fun x -> x;
```

In this example, each time you write `foo`, it's as if you had written `int -> int` instead. However, type aliases insert references to a *single* type, not textual replacement. This means that if the alias definition contains an inference variable, e.g. `alias foo = (int, _);`, then that inference variable will be shared across all uses of the alias. 

Inference variables are always based on *where they appear in the source code* and behave as if a single type was inserted at that position. This means that for inference variables in the definition of a type alias, the inferred type, whatever it is, will be the same for all uses of that alias.


## Newtype definitions

A *newtype* definition allows you to define a new, named type which is distinct from all other types, while still having the same runtime representation as another type. Newtypes improve type safety by making sure that values intended for different purposes don't get mixed up, even if their underlying representations happen to be compatible.

You can define a newtype via `type <name> = <underlying type>;`. For example:

```ml
type foo = int;
let a: foo = foo 42;
let b: int = foo$ a;
```

In this case `foo` is a new named type which has the same representation as `int`, but is considered a completely distinct and independent type for type checking purposes. Since it is a distinct type, you need to use the implicitly generated `foo` and `foo$` functions to convert between the type `foo` and the underlying type `int`.

Note that `$` is **not** an operator or special syntax in 1SubML. The `$` is just part of the implicitly generated identifier `foo$`. The generated coercion functions are ordinary values that you can freely pass around and reassign to other variable names:

```ml
let blarg = foo;
let c: foo = blarg 99;

let mobble = foo$;
let d: int = mobble c;
```

In fact, you can even use `$`s in identifiers of your own for unrelated code:

```ml
let a$ = 8 * 7;
let $b = 6 * 9;
print a$ - $b; // 2
```


## Polymorphic newtypes

Newtype definitions can also be *polymorphic*. In this case, the new type is a type *constructor* with *parameters*, and the underlying type may depend on those parameters.

For example, here's a simple polymorphic newtype wrapping a pair of values:

```ml
type pair[+A; +B] = (A, B);

let p1: pair[int; str] = pair (12, "k");
let p2: pair[float; pair[int; str]] = pair (9.99, p1);
```

And here's a similar version enforcing that both values in the pair have a single common type:

```ml
type pair[+T] = (T, T);

let p1: pair[int] = pair (12, 99);
```

## Variance

The `+` in front of the `A`, `B,` and `T` in the previous examples is a *variance* annotation.

The *variance* of a type parameter controls how subtyping relationships among the parameters affect subtyping of the type as a whole. 

For example, suppose you have an *immutable* list type `list[T]`. Should `list[int]` be a subtype of `list[any]`? It should, because `list[int]` is more specific than `list[any]`. `list[any]` is a list that can hold any kind of value, while `list[int]` can only hold ints, but since `int` is a subtype of `any`, this is still valid.

Therefore, `list[U]` is a subtype of `list[V]` whenever `U` is a subtype of `V`. We call this a *covariant* parameter, and it is written with the `+` sign as shown in the previous section.

The opposite of a covariant parameter is a *contravariant* parameter and is written with `-`. These are much less common, but sometimes come up with function types or mutability.

For example, consider `type func[-A; +B] = A -> B;`. The return type, `B` is covariant as normal. However, for the argument type `A`, this is reversed. A function that accepts a more general type as argument is a *subtype* of a function that accepts a less general type as an argument. In other words, `func[A1; B]` is a subtype of `func[A2; B]` whenever `A2` is a subtype of `A1`, the opposite direction of the covariant case. 

Finally, a parameter can be *invariant*. In this case, there are no subtyping relationships - the parameter has to match exactly. This is most common when mutability is involved. For example, if we have a *mutable* list type `list[T]`, then `T` needs to be invariant, and this is written with `^`, e.g. `type foo[^T] = {mut x: T};`.

It is also allowed to make the variance stricter than necessary. For example, in `type foo[^T] = (T, T);`, `T` is only used covariantly, so you *could* change the `^` to a `+`, and it would still compile. Marking the parameter as `^` puts more constraints on *users* of the type. For example, it means that `foo[int]` is not a subtype of `foo[any]` as if it would be if `T` were marked covariant, because invariant parameters have to match exactly.

## Recursive newtypes

Just like `let` bindings, newtype definitions can be made recursive via `rec`, and you can define mutually recursive types using `and`:

```ml
type rec list[+T] = [`Some (T, list[T]) | `None];

type rec even_list[+T] = [`Some (T, odd_list[T]) | `None]
    and odd_list[+T] = (T, even_list[T]);
```

There are two different ways to define a recursive type. The first is to use an ordinary newtype def where the underlying type is a structural recursive type:

```ml
type list[+T] = rec foo = [`Some (T, foo) | `None];
```

In this case, the underlying type is ``rec foo = [`Some (T, foo) | `None]``. There's only a single layer of newtype wrapping at the very top. Once you've unwrapped it, you get a *structural* recursive type with no further unwrapping required. This is called an *equirecursive* type.


The other option is to define a newtype using `type rec` where the underlying type itself refers to the new type being defined, like this:

```ml
type rec list[+T] = [`Some (T, list[T]) | `None];
```

In this case, the underlying type is ``[`Some (T, list[T]) | `None]``, which refers back to `list`. At each level, there's a separate layer of newtype wrapping. If you want to proceed through the list, you have to use the unwrapping coercion function again at each iteration. This is called an *isorecursive* type. 

One advantage of isorecursive newtype defs is that they can use *irregular* recursion, where the recursive references use *different* instantiations of the same type constructor. For example:

```ml
type rec tree[+T] = [`Some (T, tree[(T, T)]) | `None];
```

Notice how on the right hand side, we can use `tree[(T, T)]` (or any other instantiation), not just `tree[T]`. This is impossible to do with equirecursive types, as it would lead to an infinitely large type.

## Implicit coercions

Normally, before you can do anything useful with a value with a nominal type, you have to use the generated unwrapping coercion function to convert it back to the underlying (structural) type. However, to make newtypes more convenient to work with, 1SubML will in some cases perform unwrapping coercions *implicitly*.

```ml
type foo = {x: str};

let r = foo {x="H"};
print r.x; // H
```

In this example, `r` has the nominal type `foo`, not a record type. Normally, you would have to coerce it back to a record (via `foo$ r`) before you can do any record operations on it. However, the above example actually compiles because the `r.x` expression *implicitly* coerces `r` back to a record.


In order to avoid chaos, there are several restrictions on implicit coercions. First off, implicit coercions can only be performed when the underlying type is a record, function, or variant type. Second, implicit coercions may only be performed in specific places in the *syntax*, namely expressions which would normally require a record, function, or variant type.

Implicit record coercions may be performed by field access and field mutation expressions, as well as when matching against a record pattern (except in function arguments):

```ml
type foo = {mut x: str};

let r = foo {mut x="H"};
print r.x;   // ok, .x implicitly coerces
r.x <- "JK"; // ok, .x <- implicitly coerces
let {x} = r; // ok, record pattern implicitly coerces
```

Implicit function coercions may be performed by function call expressions:

```ml
type foo = int -> int;

let r = foo (fun x -> x + 2);
print r 23;     // ok, r is coerced to a function
print 93 |> r;  // ok, r is coerced to a function
```

Implicit variant coercions may be performed when matching against a variant pattern (except in function arguments):

```ml
type foo = [`Some int | `None];

let r = foo `Some 23;

// ok, match pattern implicitly coerces r to variant
print (match r with  
| `Some x -> x         
| `None -> "none"
);
```

Finally, implicit coercions may also be performed by [module definitions](guide_ch3.md#pinning-existential-types) and [subsumption expressions](guide_ch3.md#subtyping-and-subsumption).

Note that implicit coercions can *only* be performed in these specific situations. In particular, implicit coercions are never performed during *type checking*. If you write a type assertion, the value has to actually have that type:

```ml
type foo = {x: str};

let r = foo {x="H"};

// error: r has type foo, not {x: str}
let w: {x: str} = r; 
```

## Limitations of implicit coercions

Each expression which allows implicit coercion may only perform a *single* implicit coercion. This means that the following code does not type check:

```ml
type foo = {x: str};
type bar = {x: str};

let f = foo {x="Q"};
let b = bar {x="EWR"};

let r = if true then f else b;
print r.x; // error
```

The expression `r.x` results in a type error because `r` could be a `foo` *or* a `bar` and there is no single coercion which handles both. The coercion `foo => {x: str}` is incompatible with `bar` and vice versa.

If you need to "merge" values with distinct nominal types and then access them later like this, you should coerce them to a structural type *before* merging them. For example:

```ml
// foo$ f and bar$ b both have type {x: str}
let r = if true then (foo$ f) else (bar$ b);
print r.x; // ok
```

Additionally, implicit coercions can only be performed for types that are in scope at the point where the coercion would occur:

```ml
let f = fun r -> 
    r.x; // error, foo is not in scope here

type foo = {x: int};
print f foo {x=3};
```

Finally, function argument patterns *never* perform coercions. 

```ml
type foo = {x: int; y: int};

let f = fun {x; y} -> x - y;
// error: f expects record, not foo
print f foo {x=3; y=9};
```

If you really want implicit coercions, you can work around this by destructuring within the function *body* instead:

```ml
let f = fun args -> (
    let {x; y} = args; // ok
    x - y
);
print f foo {x=3; y=9};
```

## Pattern coercion annotations

The implicit coercions performed by pattern matching are fully inferrable (based on the inferred type of the input value), so there is never any *need* to annotate them explicitly. However, you can still add explicit annotations to avoid accidentally accepting unexpected types.

To do this, just put the expected input type before the record or variant pattern. For example:

```ml
type foo = {x: int; y: int};

let foo{x; y} = foo {x=2; y=1}; // ok
let foo{x; y} =     {x=2; y=1}; // error, expected foo not record
```

To annotate the *absence* of an implicit coercion, use `_` instead. This forces the input type to be the appropriate structural type:

```ml
type foo = {x: int; y: int};

let _{x; y} =     {x=2; y=1}; // ok
let _{x; y} = foo {x=2; y=1}; // error, expected record not foo
```

## Variant constructor shorthand syntax

Variant types can be used as the underlying type for a newtype definition just like any other type, e.g. ``type foo = [`A int | `B str];``. However, 1SubML also supports an alternate syntax for variants that additionally generates constructor functions for you.

Instead of supplying a *type* for the right hand side of the newtype defition, you write `| Tag1 t1 | Tag2 t2 | ...`. For example:

```ml
type foo = | A int | B bool | F str -> str;

let x: foo = B true;
let y: foo = F (fun x -> x);
```

The shorthand syntax is the same as a normal newtype definition, except that it additionally generates functions for constructing each variant of the enum. It is equivalent to the following version using the normal syntax:

```ml
type foo = [`A int | `B bool | `F str -> str];
let A = fun x: int :: foo -> foo `A x;
let B = fun x: bool :: foo -> foo `B x;
let F = fun x: (str -> str) :: foo -> foo `F x;
```

Note that the generated functions are just ordinary function variables, and can be freely renamed, reassigned, and passed around as values. For example:

```ml
type foo = | A int | B bool | F str -> str;

let (k, l, m) = (F, A, B);
let ttt = l;

print ttt 77; // A 77
```

The variant constructor shorthand syntax additionally allows you to specify a tag with no argument type. In this case, it generates a *value* rather than a function:

```ml
type opt_int = | Some int | None;
let x: opt_int = None;

// Equivalent to
type opt_int = [`Some int | `None];
let Some = fun x: int :: opt_int -> opt_int `Some x;
let None: opt_int = opt_int `None ();
```

As a reminder, the constructor shorthand syntax is merely a convenient wrapper over the underlying type system (aka "syntactic sugar") and does not change the actual types or behavior. In particular, the syntax used for *matching* on variant types is unaffected. You still need to prefix variant tags with "`" in patterns:

```ml
let _ = fun x: opt_int :: int -> 
    match x with 
    | `Some y -> y * 2
    | `None -> 0
;
```

If you write just "None" in a pattern instead of "`None", it will be interpreted as a variable pattern (which matches everything). Fortunately, this mistake will nearly always result in a compile error, because wildcard matches make every case after them unreachable, and having unreachable match cases is an error.

---

Previous: [Chapter 1: Expressions and values](guide_ch1.md) | Next: [Chapter 3: Polymorphism and modules](guide_ch3.md)
