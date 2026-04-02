# Chapter 3: Polymorphism and modules

[Back to Table of Contents](guide.md) | Previous: [Chapter 2: Types](guide_ch2.md) | Next: [Chapter 4: Advanced topics](guide_ch4.md)

---

Up until now, we've only dealt with *monomorphic* code, where each value has a single, specific type. However, in some cases, you may need *polymorphism* in order to facilitate code reuse.

For example, consider the identity function, `id` here:

```ocaml
let id = fun x -> x;

let _ = 1 + id 1;
let _ = 2.2 *. id -1.9;
```

`id` is a function that returns its argument unchanged and could theoretically work for any type of value. However, in monomorphic code, it has to operate on a single specific type. Therefore, if we try to use it on both `int`s and `float`s as shown above, we'll get a type error:

```
TypeError: Value is required to have type float by the *. operator expression here:
let _ = 1 + id 1;
let _ = 2.2 *. id -1.9;
               ^~~~~~~  
However, that value may have type int here:
let id = fun x -> x;
let _ = 1 + id 1;
               ^  
let _ = 2.2 *. id -1.9;
Hint: To narrow down the cause of the type mismatch, consider adding an explicit type annotation here:
let id = fun x :: _ -> x;
              +++++       
let _ = 1 + id 1;
```

In order to make this work, we need to make `id` *generic* over its input type, so it can work for both ints and floats (as well as any other type).

## Generic function definitions

To define a generic function, you need to add one or more *type parameters* after the `fun`, and then use those type parameters to annotate the type of the function, like so:

```ocaml
let id = fun[T] x: T :: T -> x;

let _ = 1 + id 1;
let _ = 2.2 *. id -1.9;
```

Here, `T` is a type parameter for the function, representing a type that can arbitrarily be chosen by the caller on a per-call basis. Then we annotate the function argument and return type to show that it takes a value of type `T` and returns a value of type `T`. Whenever the function is called, `T` will be substituted for the appropriate type, `int` or `float` in this example.

A function can also have multiple type parameters:

```ocaml
let swap = fun[A; B] (x: A, y: B) :: (B, A) -> (y, x);
print swap ("hello", false);
print swap ({x=42}, 7.8);
```

Type parameters with a function signature are never inferred during type inference because they are not true types. This means you need to specify every use of type parameters in the function's type signature explicitly. However, within the *body* of the function, the type parameters are replaced by ordinary abstract types which *can* be inferred, just like any other type.

```ocaml
let _ = fun[T] (x: T) :: _ (* inferred any *) -> (
  let y: _ (* inferred T *) = x;
  y
)
```
In the above example, the first `_` is in the function signature, where `T` is a type parameter, and hence it cannot be inferred to `T`, and instead becomes `any`. However, the second `_` appears within the body of the function, where `T` is an ordinary type and can freely be inferred.

Note that parts of the function signature which do *not* reference its type parameters are still fully inferrable and hence can be omitted.

## Generic function types

Generic function types can be written as `[type parameters]. function type`. For example, the `id` and `swap` functions above have the types `[T]. T->T` and `[A; B]. (A, B) -> (B, A)` respectively.


```ocaml
let id: [T]. T->T = 
  fun[T] x: T :: T -> x;

let swap: [A; B]. (A, B) -> (B, A) = 
  fun[A; B] (x: A, y: B) :: (B, A) -> (y, x);
```

Generic function types are just ordinary types like any other type, and hence can themselves be passed into functions. For example:

```ocaml
let f = fun[T] (v: T, f: [U]. (T, U) -> U):: (int, float) -> (f (v, 1), f (v, 9.3));
```

Generic types can be freely nested. For example, if you wanted a curried "pair" function which was as generic as possible, you could write the type as `[A]. A -> [B]. B -> (A, B)`. In this case, the outer function has one parameter `A` and returns a function, which is itself generic (with parameter `B`). 

This means that the `B` type is not chosen until the second function call, and can differ, even with the same A value. An alternative type for a curried "pair" function would be `[A; B]. A -> B -> (A, B)`, in which case both types are fixed at the point of the first function call.

## Type parameter naming

For generic functions, type parameter names are part of the function's type. This means that `[T]. T->T` and `[U]. U->U` are distinct, incompatible types. However, order does not matter. `[A; B]. (A, B) -> (B, A)` and `[B; A]. (A, B) -> (B, A)` are considered different written representations of the *same* type.

If you want to rename a type parameter without changing the function's type, you can add an *alias* using `as`. For example:

```ml
let f: [T]. T->T = fun[T as U] x: U :: U -> x;
```

In this example, the `T as U` means that the parameter's name is `T` for the purpose of its *type* (and hence the function has type `[T]. T->T`, but it is referenced in the code as `U`.

## Subtyping and subsumption

The basic rule for subtyping between polymorphic types in 1SubML is that the polymorphic part has to match exactly, while the non-polymorphic parts are subject to normal subtyping rules. (For the precise details on how this works, see [spine constructors](guide_ch4.md#spine-constructors).)

For example, `[T]. T -> (T, int)` is a subtype of `[T]. T -> (T, any)` because the `_ -> (_, int)` part does not depend on `T` and `int` is a subtype of `any`. However, `[T]. T -> (T, T)` is **not** a subtype of `[T]. T -> (T, any)` because the `_ -> (_, T)` part *does* depend on `T` and hence has to match exactly.

However, even though these types aren't *subtypes*, you can still convert between them *explicitly* using the *subsumption* operator `:>`. The basic form of the subsumption expression is `(<expr> :> <output type>)` and it lets you convert between polymorphic types that are otherwise incompatible.

For example, you can convert from `[T]. T -> (T, T)` to `[T]. T -> (T, any)`:

```ml
let f: [T]. T -> (T, T)
    = fun[T] x: T :: (T, T) -> (x, x);

let g: [T]. T -> (T, any) = (f :> [T]. T -> (T, any));
```

Note that the type annotations on `f` and `g` are unnecessary. This example just included them to demonstrate that the type was in fact converted from `[T]. T -> (T, T)` to `[T]. T -> (T, any)`. Here's the same code example with all the inferrable type annotations removed:

```ml
let f = fun[T] x: T :: (T, T) -> (x, x);

let g = (f :> [T]. T -> (T, _));
```

The output type of a subsumption expression always has to be specified explicitly (e.g. `(foo :> _)` is a compile error), since otherwise, it would not be clear which type you're even trying to convert to. However, individual *parts* of the output type can still be inferred, such as the `any` in the above example.


## Substitutions

Function type subsumption expressions allow you to optionally perform *substitutions* on the input type before it is compared to the output type. This is written as an optional `with [name=replacement]` list after the output type. For example:

```ml
let f = fun[T] x: T :: T -> x;

// With explicit replacements
let g = (f :> int -> int with [T=int]);
// With default (implicit) replacements
let g = (f :> int -> int);
```

In this example, the input type is `[T]. T -> T`. We replace `T=int` to get the type `int -> int`, which is then compared to the desired output type (also `int -> int`). In this particular example, the explicit replacement is unnecessary, because the default behavior does the same thing. However, when *renaming* a type parameter, explicit replacements are necessary.

In order to replace a type parameter with a type parameter *of the output type*, you can use the special `_.name` syntax. For example, we can convert from `[A; B]. (A, B) -> (B, A)` to `[X; Y]. (X, Y) -> (Y, X)` like so:

```ml
let f = fun[A; B] (a: A, b: B) :: (B, A) -> (b, a);

let g = (f :> [X; Y]. (X, Y) -> (Y, X) with [A=_.X; B=_.Y]);
```

In this example, we use the explicit replacement list `[A=_.X; B=_.Y]` to replace `A` with `X` and `B` with `Y`.

This special "placeholder" syntax is only allowed within the replacement list of a subsumption expression and has no relation to normal type inference variables (which are written `_`).


The placeholder syntax can be used within larger type expressions. For example, here we convert from `[T]. T->T` to `[A; B]. (A, B) -> (A, B)` by replacing `T` with `(A, B)`:

```ml
let f = fun[T] x: T :: T -> x;

let g = (f :> [A; B]. (A, B) -> (A, B) with [T=(_.A, _.B)]);
```

## Default substitutions

If the input type has a type parameter which is not mentioned in the explicit replacement list (if any), then implicit *default* replacement rules are used. The default rules are

* If the output type has a parameter with the same name and kind, replace it with that (e.g. `[T=_.T]`)
* Otherwise, replace it with a type inference variable (`[T=_]`).

For example:

```ml
let f = fun[T; U] x: (T, U) :: (T, U) -> x;

let g = (f :> [T]. (T, int) -> (T, any));
```

In this example, the input type `[T; U]. (T, U) -> (T, U)` has parameters `T` and `U`. The output type, `[T]. (T, int) -> (T, any)` has a single parameter named `T`. Since `T` matches an output parameter name, it is replaced with that parameter (`[T=_.T]`). `U` does not match an output parameter, so it is replaced with an inference variable, leading to the replacement list `[T=_.T; U=_]`.

The inferred type for U's replacement is `int`, leading to `(T, int) -> (T, int)` after replacement. This is a subtype of the output type `(T, int) -> (T, any)`, so the subsumption check passes.


## Existential types

*Existential types* are the mirror image of generic types. A generic function has type parameters which can be substitued for any type by the caller. An existential type by contrast has type parameters representing *some* unknown type that can differ on a per-value basis.

In 1SubML, existential types are tied to records. Existential record type parameters are written with `type name;`, e.g. `{type t; zero: t; plus_one: t->t}`. The only way to create values of an existential type is to use the subsumption operator to convert from an ordinary record type:

```ocaml
let r = {zero=0; plus_one=fun x->x+1}; // ordinary record value
let r2 = (r :> {type t; zero: t; plus_one: t->t}); // converted to existential type
```

In order to access values of an existential type, you need to *pin* it using the syntax `mod <name> = <expr>;`, which will be explained in more depth in the [next section](#pinning-existential-types).

```ocaml
mod M = r2;
print M.plus_one M.zero; // 1
```


The advantage of existential types is that they let you work with many values with different types, as long as the individual type of each value matches a specific pattern. 

For example, suppose we want to change the above code so that it could be working with ints or floats. 

```ocaml
let r = if false then 
  {zero=0; plus_one=fun x->x+1}
else 
  {zero=0.0; plus_one=fun x->x+.1.0}
;

print r.plus_one r.plus_one r.zero;
```

With ordinary records, this results in a type error:
```
TypeError: Value is required to have type float by the +. operator expression here:
  {zero=0; plus_one=fun x->x+1}
else 
  {zero=0.0; plus_one=fun x->x+.1.0}
                             ^       
;
However, that value may have type int here:
let r = if false then 
  {zero=0; plus_one=fun x->x+1}
        ^                       
else 
  {zero=0.0; plus_one=fun x->x+.1.0}
Hint: To narrow down the cause of the type mismatch, consider adding an explicit type annotation here:
let r: _ = if false then 
     +++                  
  {zero=0; plus_one=fun x->x+1}
else 
```

This code fails to typecheck because the incompatible types (`int` and `float`) are mixed together. The compiler sees that `r.zero` could be an `int` or `float`, while `r.plus_one` could be a function that takes an `int` or a function that takes a `float`. Thus it thinks that the `int` could be passed to a function requiring a `float` or vice versa, resulting in a type error.

Each individual branch of the `if` expression is self-consistent. The `then` branch has `zero: int` and `plus_one: int->int`, and the else branch has `zero: float` and `plus_one: float->float`. So each individual record in isolation is ok, but when they're mixed together, the compiler has no way to know this.

The way to solve this is by converting the records to an existential type (`{type t; zero: t; plus_one: t->t}`) before mixing them. This allows code to work with heterogenous types like this with no problems:

```ml
alias foo = {type t; zero: t; plus_one: t->t};

let r: foo = if false then 
  ({zero=0; plus_one=fun x->x+1} :> foo)
else 
  ({zero=0.0; plus_one=fun x->x+.1.0} :> foo)
;

mod M = r;
print M.plus_one M.plus_one M.zero;
```

## Pinning existential types

In 1SubML, `mod <name> = <expr>;` is very similiar to the ordinary let binding `let <name> = <expr>;`, but with two additional behaviors. First off, it optionally performs an [implicit record coercion](guide_ch2.md#implicit-coercions). 

Second, if the resulting type is an existential record type, it *pins* the type parameters. For each type parameter, it generates a fresh abstract type in the current scope and replaces the type parameter with the new type, resulting in an ordinary (non-existential) record type.


In this example, the input type is `{type t; zero: t; plus_one: t->t}`, with one existential type parameter, `t`. The `mod M` binding generates a new abstract type named `M.t` and then replaces `t` with `M.t`. `M.t` is an ordinary abstract type which can be freely used in type annotations:

```ml
let r: {type t; zero: t; plus_one: t->t} = ...;

mod M = r;

let x: M.t = M.zero;
let f: M.t -> M.t = M.plus_one;
```

## Record type alias members

In 1SubML, record types can contain type alias members. The most common way to create record alias members is via pinning existential types as shown above, but you can also added them explicitly:

```ml
mod M = {
    alias foo=int;
};

let w: M.foo = 324;
```

In this example, an ordinary (non-existential) record `{alias foo=int}` is assigned to `M`, so no pinning occurs. However `M.foo` is an *alias* to the type `int`. You can write `M.foo` in a type annotation, and it behaves just like if you had written `int`.

## Pinning and aliasing

It is important to understand the difference between the *type* `M.t` and the record *alias member* `M.t`. When you pin an existential type, a new type is generated, and then an alias member is added to the record with the same name that *aliases* the new type.

```ml
let _ = fun r: {type t; zero: t; plus_one: t->t} -> (
    mod M = r;
    // M is now a value with type {alias t: M.t; zero: M.t; plus_one: M.t->M.t}
);
```

This distinction becomes significant when you reassign the record value to a new variable with a different name:

```ml
let _ = fun r: {type t; zero: t; plus_one: t->t} -> (
    mod M = r;
    // M is now a value with type {alias t: M.t; zero: M.t; plus_one: M.t->M.t}

    mod M2 = M;
    // M2 is also a value with type {alias t: M.t; zero: M.t; plus_one: M.t->M.t}

    let x: M2.t = M.zero;
    let f: M.t -> M.t = M2.plus_one;
);
```

In this example, `M.t` and `M2.t` are both *aliases* that point to the same underlying type, a hidden abstract type. The underlying type is *named* `M.t` by the compiler, but there is no way to directly refer to it. You can only refer to it by using one of the record alias members.

Since `M.t` and `M2.t` are aliases of the same underlying type, they are interchangeable in type annotations. This is why code like `let x: M2.t = M.zero;` in the above example typechecks. 

By contrast, pinning a value of an existential type generates *new* types. This is true even if you pin the same value multiple times:

```ml
let _ = fun r: {type t; zero: t; plus_one: t->t} -> (
    mod M = r;
    
    mod M2 = r; // re-pinning `r`. New types are generated, unrelated to M's types

    let x: M2.t = M.zero; // type error: Expected M2.t, got M.t
);
```

Here, `mod M` and `mod M2` both generate a new hidden abstract type, and so `M.t` and `M2.t` point to different, incompatible types, resulting in a type error. The fact that both `M.t` and `M2.t` were generated by pinning the same value `r` doesn't matter. 

## Mod type annotations

Thanks to 1SubML's powerful type inference algorithm, explicit type annotations are almost never required. However, pinning an existential type can generate new types, which means that it has to be done *before* type inference runs. Therefore, a `mod` binding has to know the type of the value being bound, and the type has to be known before type inference runs, so that the compiler knows which new types if any to generate. 

Normally, this means that an explicit type annotation is required, e.g. `mod <name>: <type> = <expr>;`. However, if the type of the input value is known *without* type inference, the type annotation can be omitted.

Roughly speaking, type annotations can be omitted in the following cases:

* Record literals: `mod M = {x=4; y=6};`
* Typed expressions: `mod M = (r: <type>);`
* Subsumption expressions: `mod M = (r :> <type>);`
* Variables with known types: `let r: <type> = ...; mod M = r;`


Additionally, only the *top level* of the type has to be known, since this is what determines the existential type parameters (if any) and hence which types need to be generated. For example, `mod M: {a: _; b: _} = ...` is an acceptable type annotation. It's ok to infer the types of `a` and `b`, because the compiler knows from the top level (the `{}`), that there are no existential type parameters and hence no new types to generate. 


## Mod vs let bindings

In 1SubML, there are no separate module and value sublanguages like there are in OCaml. Module values are ordinary record values and module types are ordinary record types. `mod` bindings are just a specialized variant of `let` bindings that additionally pin existential type parameters as described previously.

However, there is one additionally difference. In order to prevent mistakes and confusing behavior, the 1SubML compiler enforces that record alias members (`M.t`) can only be accessed on variables bound with `mod`:

```ml
mod M1 = {alias foo=int};
let M2 = M1;
mod M3 = M2;

let a: M1.foo = 1; // ok
let b: M2.foo = 2; // error, M2 was bound with let, not mod
let c: M3.foo = 3; // ok
```

In this example, the alias member `M2.foo` still *exists*, it just can't be *accessed*. Rebinding it (`mod M3 = M2;`) allows `M3.foo` to be accessed again.


Additionally, there are no capitalization restrictions on identifiers in 1SubML. The code examples in this guide follow the convention of using capitalized identifiers for `mod` bindings and lowercase identifiers for `let` bindings, but this is not enforced by the compiler:

```ml
let A = 3;
let B = 12;
mod r = {A; B};
print r.A - r.B; // -9
```

## Record subsumption

The [subsumption operator](#subtyping-and-subsumption) can be used for record types as well as function types. The syntax and behavior are the same except that for record types, substitutions are applied to the *output* type rather than the *input* type as in the function case.

```ml
let r1 = {x=9; y="E"; swap=fun (a, b) -> (b, a)};

let r2 = (r1 :> {type t; x: t; y: t; swap: (t, t) -> (t, t)});

let r3 = (r2 :> {type i; type s; x: i; y: s; swap: (i, s) -> (s, i)} with [s=_.t; i=_.t]);

let r4 = (r3 :> {type s; y: s; swap: (never, s) -> (s, any)});
```

## Imports

In 1SubML, you must explicitly import files in order to use values or types from another file. The basic syntax is `import foo.bar.Baz;`, which will import from the file `foo/bar/Baz.ml`, relative to the compiler's search path (this defaults to the current directory).

You can import a file using module syntax:

```ml
import bar.Foo;

let x: Foo.t = Foo.a + Foo.b;
```

Or import specific members only:

```ml
import bar.Foo{a; b; t};

let x: t = a + b;
```

1SubML has separate namespaces for value and type bindings. However, the specific member import syntax will import both a value and a type with that name if present, and will hide any previous bindings with that name in both namespaces. E.g. if `Foo` has a type `t` and a value `t`, `import Foo{t};` will import both.

You can optionally rename imports using `as <name>`:

```ml
import Foo as Bar;
import Foo{a; b as c; d as e};
```

## Exports

Rather than using a separate `.mli` file like OCaml, 1SubML specifies exports at the end of the file:

```ml
let a = 3;
let b = "Hello";
let c = false;

export {
    a: int;
    b: str;
    // c not exported
}
```

The `export` declaration uses ordinary record type syntax, meaning that you can specify an *existential* record type with type parameters. This lets you hide the actual types of the values from users and replace them with fresh abstract types:

```ml
let zero = 0;
let plus_one = fun x->x+1;

export {
    type t;
    zero: t;
    plus_one: t->t;
}
```

This follows all the same rules for [existential types](#existential-types) and modules described previously, except that the pinning happens only once, done implicitly during export. This means that all imports of the same file will see the same abstract types.

`export` declarations support the same [substitution syntax](#substitutions) as subsumption expressions. This means that it is possible to explicitly specify the types for your existential type parameters when type checking exports. However, this is never necessary since the types are always fully inferrable.

```
export {
    type t;
    zero: t;
    plus_one: t->t;
} with [t=int]
```

## Import ordering

Imports must form an acyclic graph. There is currently no way to have multi-file cyclic dependencies in 1SubML.

If the imported file has side effects, those side effects occur before the file that imported them runs. Imports are executed in the order they appear in the file (including recursively), but at most once per imported file. 

```ml
// A.ml
print "A";
import B;
import D;

// B.ml
print "B";
import E;

// C.ml
print "C";

// D.ml
import F;
import F;
print "D";

// E.ml
print "E";

// F.ml
print "F";

// main.ml
import C;
import E;
import A;
import D;
// prints C E B F D A
```

Imports are resolved statically. An import anywhere in a file will cause the referenced file to be imported, even if it is nested in an expression which may not be executed:

```ml
// A.ml
print "A";

// main.ml
if false then 
    import A;
end;

// prints A
```

---

Previous: [Chapter 2: Types](guide_ch2.md) | Next: [Chapter 4: Advanced topics](guide_ch4.md)
