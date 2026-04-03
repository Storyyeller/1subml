# Chapter 4: Advanced topics

[Back to Table of Contents](guide.md) | Previous: [Chapter 3: Polymorphism and modules](guide_ch3.md)

---

## Order of evaluation

1SubML guarantees left-to-right evaluation order for subexpressions:

```ml
let f = fun x -> (print "f", x; x);

let _ = (print 1; f) (print 2; "q");
let _ = (print 3; "qq") |> (print 4; f);

let r = {mut x=1};

let y = (print 5; r).x <- (print 6; "w");
print y, r.x;
```

This example prints `1 2 f q 3 4 f qq 5 6 1 w`.

## Match exhaustiveness checking

In 1SubML, pattern match validation is *syntax* based rather than type based as in OCaml. The type constraints generated for a `match` expression are based only on the patterns in the match cases, and are independent of all types, explicit or inferred, and independent of all other code in the program.

For example,

```ml
match x with
| `A a -> a
| `B b -> b
| `C _ -> 32
```

will result in the constraint that `x`'s type is a subtype of ``[`A _ | `B _ | `C any]``. Depending on the types of the actual *values* passed for `x` some branches may not actually be reachable (e.g. if x is always `A` or `B`), but 1SubML does not care about that.

If a variant tag is mentioned in a `match` expression, the compiler assumes that this is because you intend to match on it and thus intend for it to be a reachable possibility. Therefore, in order to help catch programmer mistakes, the compiler will report a syntax error if a tag is handled in one "branch" of a `match` expression, but not in other branches of the same match.

For example:

```ml
let _ = fun x -> (
    match x with 
    | (`A, `X) -> 0
    | (`B, `X) -> 1
    | (`B, `Y) -> 2
);
```

In this case, `Y` is handled when the first part is `B`, but not when it is `A`. 1SubML assumes that `Y` is intended to be possible, and thus that not handling it in the `A` case is an error:

```
SyntaxError: Match expression is not exhaustive.
let _ = fun x -> (
    match x with 
    ^~~~~~~       
    | (`A, `X) -> 0
    | (`B, `X) -> 1
Note: For example, (`A, `Y) may not be covered.
```

An alternative design would have been to generate type constraints based only on what is fully covered (``([`A | `B], [`X])`` in this example). However, if the user explicitly mentioned the case `Y`, it is almost certainly going to be reachable at some point and silently throwing it away just leads to confusing type errors later on, so 1SubML did not go with this approach.


*Wildcard* patterns, however, are handled differently. For example, consider
```ml
let _ = fun x -> (
    match x with 
    | (`A, `X) -> 0
    | (`B, _) -> 1
);
```

Maybe the user intended the `_` part to also cover `Y`, but there's no particular reason to think so. Wildcard patterns are often used to just cover a specific set of tags, not every possible tag including tags not mentioned in the source code.

Therefore, if a match pattern doesn't handle wildcards in all "branches", they're just narrowed to the tags that *are* handled. A set of patterns like ``| (`A, `X) | (`B, _) `` as above results in the type constraint ``([`A | `B], [`X])``. 

Meanwhile, if the wildcards *are* used in all "branches", they appropriately cover everything. So a pattern set like ``| (`A, _) | (`B, _) `` results in the type ``([`A | `B], any)``.

## Decomposable patterns

Fully precise match exhaustiveness checking is NP-Hard, meaning there is no known way to do it in polynomial time. In order to keep compilation fast, 1SubML uses an algorithm which offers fully precise exhaustiveness checking for common cases but in pathological cases, it uses an approximation that could sometimes lead to "false positive" exhaustiveness errors.

Specifically, a set of match patterns is *decomposable* if it can be represented as a *tree* of decision nodes where each match case appears once as a leaf. The different subtrees can check different tags or check them in different orders, as long as it is still a tree. Additionally, if at any point, one of the patterns is an unrestricted wildcard match, then that entire subtree is considered ok, regardless of what other patterns are present under it.

Most match patterns used in practice are decomposable, but it is possible to construct pathological patterns which are not decomposable and would take exponential time to precisely check. If you have a non-decomposable pattern and run into a "false positive" exhaustiveness error, you need to split cases and/or add wildcards until it is decomposable. (Note that adding a full wildcard match makes it trivially decomposable.)

Fortunately, if you somehow run into this case, the compiler will suggest a match case you could split to help make the match more decomposable. For example:

```ml
let f = fun x -> match x with 
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
| (_, `T, `T) -> 2
| (`F, `F, _) -> 3
| (`F, _, `F) -> 4
| (_, `F, `F) -> 5
;
```

```
SyntaxError: Match expression is not exhaustive.
let f = fun x -> match x with 
                 ^~~~~~~       
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
Note: For example, (_, `F, `T) may not be covered.
Note: That case is covered by the match pattern here, but precise exhaustiveness checking requires decomposable patterns. Consider splitting this match pattern into multiple cases.
| (`T, _, `T) -> 1
| (_, `T, `T) -> 2
| (`F, `F, _) -> 3
  ^~~~~~~~~~~      
| (`F, _, `F) -> 4
| (_, `F, `F) -> 5
Hint: Split the above case into multiple cases by adding explicit tags at position (_, _, `<TAGS HERE>).
```

In some cases, you may have to split multiple cases to make the match decomposable. For example, after splitting the ``| (`F, `F, _) -> 3`` match arm following the instructions above,

```ml
let f = fun x -> match x with 
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
| (_, `T, `T) -> 2
| (`F, `F, `T) 
| (`F, `F, `F) -> 3
| (`F, _, `F) -> 4
| (_, `F, `F) -> 5
;
```

We get a new error pointing to a different match case:

```
SyntaxError: Match expression is not exhaustive.
let f = fun x -> match x with 
                 ^~~~~~~       
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
Note: For example, (`F, `T, `T) may not be covered.
Note: That case is covered by the match pattern here, but precise exhaustiveness checking requires decomposable patterns. Consider splitting this match pattern into multiple cases.
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
| (_, `T, `T) -> 2
  ^~~~~~~~~~~      
| (`F, `F, `T) 
| (`F, `F, `F) -> 3
Hint: Split the above case into multiple cases by adding explicit tags at position {_0: `<TAGS HERE>}.
```

In fact, in pathological cases, you may have to split an *exponentially large* number of cases to make it decomposable. This is the reason why the compiler doesn't just do it for you automatically.

A much simpler and more realistic approach is to just add a wildcard `_` pattern:

```ml
let f = fun x -> match x with 
| (`T, `T, _) -> 0
| (`T, _, `T) -> 1
| (_, `T, `T) -> 2
| (`F, `F, _) -> 3
| (`F, _, `F) -> 4
| (_, `F, `F) -> 5
| _ -> 999999 // will never happen
;
```

Adding a wildcard like this automatically makes any `match` decomposable without any need to split up match cases. The downside is that it requires adding a match case to the end which is never actually reachable.

## Match reachability checking

It is also a syntax error if one match case makes a subsequent match case unreachable:

```ml
let f = fun x -> match x with 
| `A -> 0
| `A -> 1
;
```

```
SyntaxError: Match case is unreachable.
let f = fun x -> match x with 
| `A -> 0
| `A -> 1
  ^~      
;
Note: All values are covered by previous match case here:
let f = fun x -> match x with 
| `A -> 0
  ^~      
| `A -> 1
;
```

The compiler only checks for whether a *single* case makes another case unreachable. If a case is unreachable due to a combination of previous cases that each cover only part of the possibilities, there is no compile error. Doing fully precise reachability checking like that would require exponential time in pathological cases, just like exhaustiveness checking. Furthermore, *in practice* unreachable match cases will always be covered by a single case anyway.


## Higher kinded types

Recall that a [newtype definition](guide_ch2.md#polymorphic-newtypes) can have type parameters, e.g. `
type baz[+T] = (float, T);`.

In this example, the parameter is a simple type, but it doesn't have to be. Newtypes can have parameters that are *themselves* type constructors which take parameters, and so on. For example:

```ml
type foo[+B[+]] = (B[int], B[str]);

type bar[+T] = T;
let x: foo[bar] = foo (bar 1, bar "p");

type baz[+T] = (float, T);
let x: foo[baz] = foo (baz (0.9, 9), baz (-4e5, "w"));
```

These are known as *higher kinded types*. When using higher kinded types, you need to specify the *kind* of each type parameter explicitly.

The **kind** of a type is simply the list of parameters (if any), and the [variance](guide_ch2.md#variance) and kind of each parameter.

Simple types like `int` don't take any parameters, and so have kind `[]`. However, when the kind is `[]`, it can be omitted.

The next level up are ordinary generic type constructors like `bar` and `baz` in the above example. `bar` takes a single parameter with variance `+` and kind `[]`, so its kind is `[+[]]`, or just `[+]` for short (since `[]` can be omitted).

Finally, in the above example `foo` takes a single parameter with variance `+` and kind `[+]`, so `foo`'s kind is `[+[+]]`. 


In addition to newtypes, [generic function definitions](guide_ch3.md#generic-function-definitions), generic function types, and [existential record types](guide_ch3.md#existential-types) can all have higher kinded parameters as well. For example:

```ml
let _ = fun[B[+]] f: ([T]. T -> B[T]) :: (B[int], B[bool]) -> (f 1, f false);
```

There is no partial application of type constructors in 1SubML. If you provide type parameters to a type constructor, you have to provide a type for *every* parameter and the resulting type always has kind `[]`. For example, if you have `type pair[+A; +B] = (A, B);`, then writing `pair[str]` is an error. However, you can allow the types to be inferred via `_`, e.g. `pair[str; _]`.

## Higher kinded type inference

Higher kinded types are still fully inferrable in 1SubML, just like ordinary types. However, when creating a higher kinded type inference variable, you will usually need to supply an explicit kind annotation via `as <kind>`. For example:

```ml
type pair[+A; +B] = (A, B);

// _ as [+;+] is inferred to be pair here
let p: _ as [+;+] [str; int] = pair ("", 3);
```

In addition to `_`, the special type expressions `any` and `never` can likewise produce types of any kind, but in most cases you need to provide an explicit kind annotation:

```ml
let p: any as [+;+] [str; int] = pair ("", 3);
```

Kinds must always match *exactly*. While 1SubML has subtyping, it does not have "subkinding". This is true even if the conflicting types are both `any`/`never`:

In this example, there is a type error between two `any`s of different kinds when assigning `y` to `z`:

```ml
let _ = fun x -> (
    let y: any as [^] [int] = x;
    let z: any as [+] [int] = y; // type error
);
```

## Spine constructors

In 1SubML, polymorphic types (i.e. [generic functions](guide_ch3.md#generic-function-definitions) and [existential record types](guide_ch3.md#existential-types)) are implemented under the hood as implicitly generated type constructors called *spine constructors*.

The spine constructor's type "contains" the parts of a polymorphic type that depend on the polymorphic type parameters, while all parts of the type that are *free* (not dependent on polymorphic type parameters) get replaced with parameters of the spine constructor.

For example, if you write `let f: [T]. (T, int) -> (T, any) = ...`, that is roughly equivalent to the following pseudocode, where `spine1` is the generated spine constructor:

```ml
type spine1[-p1; +p2] = [T]. (T, p1) -> (T, p2);
let f: spine1[int; any] = ...;
```

The `int` and `any` parts get replaced by spine constructor parameters since they don't depend on `T`.

Unlike normal type constructors, spine constructors are distinguished by their contents, not their name (in fact, they don't *have* a name). Two polymorphic types with structurally identical polymorphic parts will result in the *same* spine constructor.

For example, if you have 1SubML code like this:

```ml
let _ = fun f: ([T]. (T, any) -> (T, str)) -> (
    let g: [T]. (T, int) -> (T, any) = f;
);
```

It would desugar to something like this with spine constructors:


```ml
type spine1[-p1; +p2] = [T]. (T, p1) -> (T, p2);

let _ = fun f: spine1[any; str] -> (
    let g: spine1[int; any] = f;
);
```

The type annotations on `f` and `g` both use the same spine constructor since the polymorphic parts are identical. Therefore, `f` has type `spine1[any; str]` while `g` has type `spine1[int; any]`. Since `spine1[any; str]` is a subtype of `spine1[int; any]`, this code typechecks successfully.


## Spine constructors for nested types

For *nested* polymorphic types, each level independently gets converted to a spine constructor. For example, consider the nested polymorphic annotation on `f` here:

```ml
type foo[+A; +B] = (B, A);
let f: [T]. foo[T; (int, int)] -> [U]. (any, T, U) -> (T, foo[any; U]) = ...;
```

In the first step, the `[U]. (any, T, U) -> (T, foo[any; U])` part gets replaced by a spine constructor:

```ml
type spine1[-p1; -p2; +p3; +p4[+;+]; +p5] = 
    [U]. (p1, p2, U) -> (p3, p4[p5; U]);

type foo[+A; +B] = (B, A);
let f: [T]. foo[T; (int, int)] -> spine1[any; T; T; foo; any] = ...;
```

The new type `[T]. foo[T; (int, int)] -> spine1[any; T; T; foo; any]` then gets replaced by a second spine constructor:

```ml
type spine1[-p1; -p2; +p3; +p4[+;+]; +p5] = 
    [U]. (p1, p2, U) -> (p3, p4[p5; U]);

type spine2[-p1[+;+]; +p2; +p3[-;-;+;+[+;+];+]; -p4; +p5[+;+]; +p6] =
    [T]. p1[T; p2] -> p3[p4; T; T; p5; p6];


type foo[+A; +B] = (B, A);
let f: spine2[foo; (int, int); spine1; any; foo; any] = ...;
```

Notice how when generating `spine1`, `T` is a free type that results in a spine constructor parameter just like any other. Meanwhile, when generating `spine2`, `spine1` is a free type that results in a (higher kinded) spine constructor parameter.

Additionally, notice how free types get replaced by parameters even if those types are type constructors (like `foo`) or complex types (like `(int, int)`).

## Parameter generation of spine constructors

Notice how in the previous example, the final type was `spine2[foo; (int, int); spine1; any; foo; any]`. Every *source location* subtree in the type expression that contains a free type gets replaced by a separate spine parameter, even if they are identical to other subtrees in that were replaced. In the previous example, both instances of `foo` and both instances of `any` in `spine2` got replaced by separate parameters.

This also applies when using invariant parameter shorthand syntax or not. For example `[T]. {x: T; mut y: int} -> {x: T; mut y: int <- int}` gets turned into

```ml
type spine1[^p1; +p2; -p3] = [T]. {x: T; mut y: p1} -> {x: T; mut y: p2 <- p3};
let f: spine1[int; int; int] = ...;
```

On the left hand side, there's a single `int` so it gets replaced by the invariant parameter `p1`. On the right hand side, the explicit pair syntax `int <- int` is used, so both parts get replaced by individual parameters, `p2` and `p3` respectively.


The order of spine parameter generation is in-order tree traversal, left to right and top to bottom. However, the trees are normalized by sorting field and tag names first.

For example, `[T]. {c: int; b: str; a: T} -> T` gets turned into `type spine1[-p1; -p2] = [T]. {a: T; b: p1; c: p2} -> T; spine1[str; int]`. `str` becomes the first parameter and `int` the second, even though `str` appears after `int` in the original source code, because it comes first after the record fields are sorted.

Additionally, polymorphic parameter ordering and recursive type names don't matter for spine constructor identity. For example, `[A; B]. (A, B) -> (A, B)` and `[B; A]. (A, B) -> (A, B)` result in the same spine constructor and the same type. Likewise, `[T]. T -> rec a=(T, a)` and `[T]. T -> rec b=(T, b)` result in the same type.


## Removal of unused polymorphic parameters

Unused polymorphic parameters are removed prior to spine construction. This means that `[T]. int -> int` is the same type as `int -> int` and `[A; B]. B -> B` is the same type as `[B]. B -> B`.

Additionally, for polymorphic functions, parameters which are only used covariantly are replaced by `never` and parameters which are only used contravariantly are replaced by `any`. For example, `[A; B; C]. (A, B) -> (B, C)` is equivalent to `[B]. (any, B) -> (B, never)`.

For polymorphic records, this is swapped. Covariant-only parameters are replaced by `any` and contravariant-only parameters are replaced by `never`. Additionally, when a parameter is removed from a polymorphic record, an alias member of the same name is added. This means that `{type a; type b; f: a -> b}` becomes the type `{alias a: never; alias b: any; f: never -> any}`.

## Invariant pairs

1SubML's type system is *polarized*, meaning that type parameters are always covariant or contravariant. In the underlying type system, there is no such thing as invariant type parameters. 

For convenience, 1SubML allows you to specify invariant parameters on [newtype definitions](guide_ch2.md#newtype-definitions). These are converted to a *pair* of parameters under the hood, one covariant and one contravariant.

Recall that `{mut x: int}` is shorthand for `{mut x: int <- int}`, where the first `int` indicates the type that can be read from the field and the second `int` indicates the type that can be *written* to the field. The same `<-` syntax can also be used with invariant parameters:

```ml
type foo[^T] = {mut x: T};

let r: foo[any <- int] = foo {mut x=""};
```

`type foo[^T] = {mut x: T};` is roughly equivalent to `type foo[+T1; -T2] = {mut x: T1 <- T2};` except that with `^`, the parameter is specified as a *single* parameter syntax wise, e.g. `foo[int]` instead of `foo[int; int]`. If you want to explicitly specify both halves of the invariant pair, you would instead write `foo[int <- int]` as shown above.

## Type witnesses

In 1SubML, `a -> b` is the type of arbitrary functions from `a` to `b`. However, there is also `a => b`, which is the subtype of functions from `a` to `b` that are guaranteed to be *pure identity functions*. 

Since `a => b` is always an identity function, the existence of a value of type `a => b` is proof that the underlying type of `a` is a subtype of the underlying type of `b`. Essentially, it is a *witness* of type equality.

There are only two ways to create values with `=>` types. The first are the wrap/unwrap functions implicitly generated for [newtype definitions](guide_ch2.md#newtype-definitions). For example, when you write `type foo = int;`, in addition to the type constructor, this implicitly generates a wrapper function named `foo` with type `int => foo` and an unwrapper function named `foo$` with type `foo => int`. Likewise, `type bar[+T] = (T, T);` results in a value `bar` of type `[T]. (T, T) => bar[T]` and a value `bar$` of type `[T]. bar[T] => (T, T)`.

The second method is via the special expression `id!`. This creates a value of type `t => t` for any (inferred) type `t`. 

```ml
let a: int => int = id!;
let b: str => str = id!;
```

You can also specify the type explicitly via `[_]`. For example `id![int]` has type `int => int`. However, the type is fully inferrable, so this is never strictly necessary. When you omit the `[]` part, the type is inferred from context like normal. In other words, `id!` is equivalent to `id![_]` (which uses an explicit type inference variable `_`).


## Variance dependent pair syntax

In the particular case of polymorphic function types, 1SubML supports special syntax for a type that is effectively a *pair* of type variables. Specifically, if `A` and `B` are parameters of the polymorphic type, then the syntax `A/B` is a special type that is `A` when used covariantly and `B` when used contravariantly.

Under normal circumstances, you could just write `A` or `B`, so this only makes sense when the syntax appears in an invariant position. For example, `[A; B]. {mut x: A/B} -> A -> B`. 

This is roughly equivalent to `[A; B]. {mut x: A <- B} -> A -> B`, except that it allows you to write `A/B` in a single position, which matters in more complex cases. For example, in `[A; B]. {mut x: (A/B, int)} -> A -> B`, there's a single `int`, whereas the alternative `[A; B]. {mut x: (A, int) <- (B, int)} -> A -> B` requires duplicating the `int`, resulting in an extra spine constructor parameter.

This syntax is *only* supported for function *types*, and is not supported for record types, function *definitions*, or newtype definitions. The only reason this syntax exists is because it is required to express the types of the wrapper and unwrapper functions implicitly generated for [newtype definitions](guide_ch2.md#newtype-definitions) in certain cases.

For example:

```ml
type foo[^T] = {mut x: T};

let _: [T; T$]. {mut x: T/T$} => foo[T$ <- T] = foo;
let _: [T; T$]. foo[T <- T$] => {mut x: T/T$} = foo$;
```

In this example, the type of the generated `foo$` function is specifically `[T; T$]. foo[T <- T$] => {mut x: T/T$}`, which is a distinct type from `[T; T$]. foo[T <- T$] => {mut x: T <- T$}`.



Additionally, when pruning unused polymorphic variables prior to spine construction, the removal is skipped if it would require placing part of a pair type. For example in the type `[A; B]. A -> {mut x: A/B}`, `B` is only used contravariantly, so it would normally get replaced by `any`. However, `B` is kept in this case since replacing it would require replacing part of the `A/B` pair.

## Explicit registration of implicit coercions

For [newtype definitions](guide_ch2.md#newtype-definitions), [implicit coercions](guide_ch2.md#implicit-coercions) for the new type construction are registered automatically. However, fresh types can also be created by [polymorphic function definitions](guide_ch3.md#generic-function-definitions), [`mod` bindings](guide_ch3.md#pinning-existential-types), and file [exports](guide_ch3.md#exports). In these cases, there are no automatically registered implicit coercions, but you can still register implicit coercions for the new types explicitly via an `implicit` declaration.


Here's an example of `implicit` for function definitions:

```ml
let f = fun[C] implicit{C: Q _} (c: C, Q: (C => {x: int})) -> 
    c.x // implicit coercion allowed here
;

type foo = {x: int; y: str};
let b = foo {x=11; y=""};

print f (b, foo$); // prints 11
```

For `mod` bindings:

```ml
type T = {x: int};
let b = T {x=12};
let m = ((T$, b) :> {type B; _0: B => {x: int}; _1: B});
mod M implicit{B: _0 _} = m;
print M._1.x; // 12
```

And for file `exports`:

```ml
type T = {x: int};
let qwe = T$;
let x = T {x=2};

export {
    type A;
    qwe: A => {x: int};
    x: A;
} implicit{A: qwe _}
```

## Implicit declarations

The format for implicit declarations is `implicit{<coercions>}`, where `<coercions>` is a semicolon list of `<type>: <unwrap> <wrap>` triples. `<unwrap>` is the desired unwrapping coercion for `<type>` and so on. Either spot can be omitted by writing `_` instead as above. For example, `implicit{A: qwe _}` means that `qwe` will be used as an implicit unwrapping coercion for `A`, while there is no implicit wrapping coercion registered.

When explicitly registering an implicit coercion, the following requirements must be met:

1. The type and coercion values must be defined at the point (function def, `mod` binding, or `exports`) where the `implicit` declaration appears.
2. The coercions must be `=>` types i.e pure identity functions.

We call the *source* type the `foo` or `foo[p1; p2; ...]` part, where `foo` is the type the coercion is being registered for, and the *target* type the other half. An unwrapping coercion must have type `<source> => <target>` and a wrapping coercion must have type `<target> => <source>`.

3. The source type must cover all possible values of the specified type. This means that if it has parameters, those parameters must be `any`/`never` or polymorphic type parameters.
* Example: for `foo` with kind `[+;-]`, `foo[any; never] => int -> int` or `[A; B]. foo[A; B] => B -> A` would both be acceptable. However, `foo[int; int] => int -> int` would not be acceptable because the source type doesn't cover all possible parameterizations of `foo`.
4. All polymorphic type parameters if any must be unique determined by the source type. This means that they appear exactly once in the source type and appear in the target type only with opposite polarity.
5. For unwrapping coercions, the target type must be a (possibly polymorphic) function or record type, a variant type, or `never`. For wrapping coercions, the target type must be a (possibly polymorphic) function or record type.

Finally, the type of the coercion must be known (before type inference), at least enough to verify the above properties. This normally means that it needs to have an explicit type annotation.

## constructor-of!

The type expression `constructor-of!(<type>)` evaluates to the *type constructor* of `<type>`. (It is an error if `<type>` evaluates to a type is not a type constructor application.)  For example:

```ml
type foo[+T] = (T, T);

// equivalent to alias f = foo;
alias f = constructor-of!(foo[int]);
let v: f[str] = foo ("Hello", "World");
```

For ordinary type constructors, this is not terribly useful because you could have just written `alias f = foo;` instead. However, for *spine constructors*, it is useful because there is no other way to refer to spine constructors:

```ml
alias foo = constructor-of!([T]. (T, any) -> (T, any));

let f: foo[str; str] = fun[T] x: (T, str) :: (T, str) -> x;
let f: foo[any; int] = fun[T] x: (T, _) :: (T, _) -> (x._0, 12213);
```

## Embedded Javascript

You can directly embed Javascript in the output program using the special expression `js!%<js code>%`. This allows you to access functionality that could not otherwise be implemented in 1SubML. For example, here is the source code for the standard library `Dyn` module:

```ml
import Std{dyn};

let new = js!%() => {let s = Symbol(); let wrap=t => [s, t]; let unwrap=([a, b]) => (a === s) ? {_:'Some', $: b} : {_:'None'}; return {wrap, unwrap}}%;

export {
    alias t: dyn;

    new: [T]. any -> {wrap: T -> dyn; unwrap: dyn -> [`Some T | `None]};
}
```

You can see the other standard library modules for other examples of embedded Javascript.

The embedded code is included in the compiled output as-is with absolutely no checks, so it is the programmer's responsibility to ensure that the code is well-formed and adheres to the requirements that 1SubML expects.

The type of a `js!` expression is `never`, meaning there is no type checking by default. The programmer is responsible for adding an appropriate type annotation to prevent users from accidentally misusing the resulting value.

---

Previous: [Chapter 3: Polymorphism and modules](guide_ch3.md)






