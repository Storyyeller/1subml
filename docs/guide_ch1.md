# Chapter 1: Expressions and values

[Back to Table of Contents](guide.md)

## Binary operators and literals

1SubML has the primitive types `bool` (`true` or `false`), `int` (arbitrary precision integers), `float` (64 bit floating point), and `str` (strings).

The integer operators are `+`, `-`, `*`, `/`, `%`, `<`, `<=`, `>`, and `>=`. For floating point operations, suffix the operator with `.`, e.g. `1.1 +. 2.2`. The equality operators `==` and `!=` accept values of any type, but values of different types compare nonequal. String concatenation is `^`.

```
>> 5 + 77
82
>> 8.1 *. 1.1
8.91
>> "Hello, " ^ " World!"
"Hello,  World!"
>> 7 < -99
false
>> -9.9 <. 1242.1e3
true
>> 9 == 9.0
false
>> 10 == 10
true
>> 5.4 != "5.4"
true
```

For non-primitive values, equality comparison is by object identity:

```
>> let a = {}
{}
>> a == a
true
>> a == {}
false
```

## Comments

1SubML has both line comments with `//` and block comments with `(* *)`. Block comments cannot be nested.

```ml
// comment
(* multi
line
comment *)
```

## Let bindings

`let` introduces a new variable. Statements are separated by semicolons:

```ml
let x = 5;
let y = x + 1;
print y;           // 6 
```

Variables can never be reassigned, but they can be *shadowed* by adding a *new* variable with the same name:


```ml
let x = 5;
let y = x + 1;
let x = 99; // x is now 99, not 5
print (x, y);           // (99, 6)
```

The `print` statement prints one or more expression values to the console. Values of any type can be printed, and you can print multiple expressions by separating them with commas. If the output is long, it will be truncated to avoid spamming the console.

```ml
print 1 - 2, true, "Hello" ^ " world!"; // -1 true Hello world!
```


## Block expressions

1SubML is expression-oriented -- a block of semicolon-separated expressions evaluates to its last expression:

```ml
let result = (
    let a = 10;
    let b = 20;
    a + b           // this is the value of the block 
);
print result;       // 30 

```

You can also group expressions using `begin ... end` instead of `( ... )`:

```ml
let result = begin
    let a = 10;
    let b = 20;
    a + b
end;
```

Block expressions are ordinary expressions and can thus be used anywhere an expression can appear, even in the middle of arithmetic or other code:

```ml
print (4 * (let x = 2; x - 7)) + 1; // -19
```


Block expressions introduce a new scope, so variable bindings introduced within the parentheses are not visible outside of them:

```ml
let x = 4;
let y = (
    let x = x + 3;
    x * x
); // the "let x = x + 3" goes out of scope here.
print (x, y); // prints (4, 49), not (7, 49)
```

You can also write block expressions that contain *only* statements with no final expression. In that case, the block expression evaluates to an opaque value. Since the expression value can't be used for anything, this is normally used for code that has side effects.

```ml
let _ = (
    print "Hi";
    print "There";
    print "Look, no trailing expression!";
);
```


## If expressions

In 1SubML `if` is an expression, not a statement. The general form is `if <expr> then <expr> else <expr>`. For example, evaluating `if false then "Hello" else "World"` would result in `"World"`. You can think of this as similar to the ternary operator (`a ? b : c`) in C-style programming languages.

You can also have `if` expressions with no `else` branch. In this case, you need to end it with `end`, and the expression evaluates to an opaque value. Since the expression value can't be used for anything, this is normally used for code that has side effects.

```ml
if true then (
    print "Some stuff";
    print "with side effects";
) end; // no else branch
```

## Records

Records are a grouping of zero or more named values similar to "objects" or "structs" in other programming languages and are defined via `{name1=val1; name2=val2; ...}`. You can access the value of a field using the usual `.name` syntax. For example `{a=true; b="Hello"; c={}}.b` evaluates to `"Hello"`.

There is a special shorthand syntax for fields with the same name as their value - `{a; b; c=4}` is equivalent to `{a=a; b=b; c=4}`.

Records in 1SubML are anonymous and *structurally typed*. A record with more fields is a subtype of a record with fewer fields, leading to a "compile time duck typing" effect.

For example, this code typechecks even though the branches of the `if` expression are records with different sets of fields:

```ocaml
let x = if true then 
        {a=4; b=2} 
    else 
        {c=9; a=1};
x.a
```

However, everything is still statically typed. Attempting to access a possibly undefined field results in a compile error:

```ocaml
let x = if true then 
        {a=4; b=2} 
    else 
        {c=9; a=1};
let _ = x.b; // TypeError: Missing field b.
```

## Mutable fields

In 1SubML, variables cannot be reassigned, merely shadowed. The only way to have truly mutable state is via *mutable record fields*. When creating a record value, you can optionally make fields mutable by prefixing them with `mut` and you can update the value of mutable fields via `record.name <- new_value`:


```ocaml
let x = {a=4; mut b=6; c=9};
print x; // {a=4; b=6; c=9}
x.b <- x.b + 11;
print x; // {a=4; b=17; c=9}
```

Records use reference semantics, so mutating a field affects all references to the same object, even if they are bound to different variable names:


```ocaml
let x = {mut i=1};
let y = x;
let z = {mut i=1};
print x, y, z; // {i=1} {i=1} {i=1}

y.i <- 32;
print x, y, z; // {i=32} {i=32} {i=1}
```

Record field assignment is itself an expression. Unlike C-family languages, where assignment expressions evaluate to the *new* value and unlike Rust and ML-family languages where it evaluates to `()`, in 1SubML, assignment evaluates to the *old* value (the one being replaced). This is generally more useful, since it lets you easily swap values:


```ocaml
let x = {mut a=3; mut b=8};
print x; // {a=3; b=8}

x.a <- x.b <- x.a;
print x; // {a=8; b=3}
```


## Tuples

You can create *tuples* via `(a, b, c, ...)` and access specific fields of a tuple via `._0`, `._1`, etc. For example `(1, 3, 5, 7, 9)._3` is `7`.

In fact **tuples are just a special case of records**. The expression `(a, b, c)` is just a shorthand syntax for the record expression `{_0=a; _1=b; _2=c}`. Since tuple values are just record values, you can freely mix and match the shorthand and explicit syntax.

There is no shorthand syntax for 0 or 1 element tuples. For that, you have to use the explicit record syntax (`{}` or `{_0=x}` respectively). Furthermore, tuple syntax does not support mutable fields. If you need mutable fields, you have to use the full record syntax described above.


```ocaml
let foo = {_4=true; _2="a"; mut _1="b"; _0="c"; _3=42};
print foo; // ("c", "b", "a", 42, true)

foo._1 <- 9.7;
print foo; // ("c", 9.7, "a", 42, true)
```

## Destructuring assignment

In order to more conveniently access multiple fields of a record value, you can *destructure* records when assigning them. For example, instead of doing `let foo = record.field1; let bar = record.field2; let baz = record.field3` etc., you can just write `let {field1=foo; field2=bar; field3=baz} = record;` instead.

As with record value construction, you can omit the `=` part when the field and variable name match. For example, `let {field1=foo} = record` is equivalent to `let foo = record.field1`, while `let {foo} = record` is equivalent to `let foo = record.foo`.

You can mix and match the shorthand and full field assignment syntax:

```ocaml
let x = {a=2; b=7; c=9};

let {c; a=foo} = x;
print foo, c; // 2 9
```

Additionally, record destructuring can be nested. The left hand side of an assignment is actually a *pattern*, not just a variable name. The simplest kind of pattern is a *variable pattern*, which just binds a single variable (e.g. `let a = ...` binds the variable `a`), but patterns can also be *record patterns* as described above. 

When using the `{field=pattern}` syntax in a record pattern, the right hand side can be *any pattern*, including variable or record patterns, meaning that patterns can be arbitrarily nested:


```ocaml
let record = {a=3; b=4; c={d=5; e=6; f={a=90}}};
let {a; b=x; c={d; e=y; f={a=z}}} = record;
print a, x, d, y, z; // 3 4 5 6 90
```

Patterns can also use tuple shorthand syntax:

```ocaml
let tup = (42, true, "hello");
let (a, b, c) = tup;
```

Variable patterns with the name `_` do not bind a variable, and instead just ignore the matched value. This is mainly useful in tuple patterns when you don't care about some of the values:


```ocaml
let tup = (42, true, "hello", 9.1);
let (a, _, _, c) = tup;

print a; // 42
print c; // 9.1
```

## Functions

Functions are defined with `fun` and `->`:

```ml
let add_one = fun x -> x + 1;
```

Function application uses juxtaposition (just put the argument after the function) -- no parentheses needed:

```ml
print add_one 3;    // 4
```

Function application is **right-associative**. `a b c` parses as `a(b(c))`. This means that you don't need parentheses to apply multiple functions:

```ml
print add_one add_one 3;    // 5
```

You can also call functions using the reverse application operator `|>`. `a |> b` is equivalent to `b a` except that `a` will be evaluated before `b`. Additionally, the `|>` operator is left-associative, meaning that `a |> b |> c` parses as `(a |> b) |> c`, which is in turn equivalent to `c b a`.

```ml
print 3 |> add_one |> add_one; // 5
```


## Multiple arguments

All functions in 1SubML take exactly one argument. In order to define functions that take multiple arguments, you can just use a tuple as the function argument instead:

```ocaml
let sub = fun (a, b) -> a - b;
print sub (2, 5); // -3
```

You can also simulate named arguments by taking a record instead:

```ocaml
let sub = fun {a; b} -> a - b;
print sub {a=2; b=5}; // -3
print sub {b=5; a=2}; // -3
```

In fact, the function argument can be any pattern, so you can even nest patterns if you want:
```ocaml
let f = fun {a; b={c=x; d}} -> a + x * d;
print f {b={c=4; d=7}; a=12}; // 40
```

An alternative approach to multiple argument functions is *currying*, where the original function just returns another function until all arguments have been applied:

```ocaml
let sub = fun a -> fun b -> a - b;
print (sub 2) 5; // -3
```

However, currying is not recommended because it is more verbose and leads to confusing type error messages if you mess up the arguments. Furthermore, you can't simulate named arguments the way you can if you take a record as the function argument.

## Recursive let bindings

Sometimes, one wishes to have functions that call themselves recursively. Unfortunately, this is impossible with the above constructs since plain let-expressions can only refer to variables that are already defined. 

In order to support recursion, 1SubML offers _recursive let expressions_ which are defined via `let rec` and allow the definition of the variable to refer to itself. For example, you could define a recursive fibonacci function as follows:

```ocaml
let rec fib = fun x ->
    if x <= 1 then 
        1
    else
        fib(x - 1) + fib(x - 2)
```

In order to avoid code referring to variables that don't exist yet, the right hand side of `let rec` variable definitions is restricted to be a function definition.


## Mutual recursion

The above syntax works for a single function that refers to itself, but in some cases, you may want to have multiple functions that each refer to each other. Unlike in the case with `let`, simply nesting `let rec`s won't work. Therefore, `let rec` allows _multiple_ variable bindings, separated by `and`. For example, you can define mutually recursive `even` and `odd` functions as follows:

```ocaml
let rec even = fun x -> if x == 0 then true else odd(x - 1)
    and odd = fun x -> if x == 0 then false else even(x - 1)
```


## Variants and match expressions

Sometimes you need to make different decisions based on runtime data in a type safe manner. 1SubML supports this via _variants_, also known as _sum types_ or _enums_. Basically, the way they work is that you can wrap a value with a tag, and then later match against it. The match expression has branches that execute different code depending on the runtime value of the tag. Crucially, each match branch has access to the static type of the original wrapped value for that specific tag.

To wrap a value, prefix it with a grave (`` ` ``) character and an identifier tag, e.g. `` `Foo 42``.

As with records, you can destructure variant values using variant patterns:

```ocaml
let x = `Foo {a=42};
let `Foo {a} = x;
print a; // 42
```

However, this isn't terribly useful, since it is limited to variant values that statically have only one possible variant. In order to do anything useful with variants, you need to use a *match expression*.

Match expressions list one or more possible cases, and an expression to execute in each case. For example:

```ocaml
let calculate_area = fun shape ->
    match shape with
    | `Circle {rad} -> rad *. rad *. 3.1415926
    | `Rectangle {length; height} -> length *. height;

calculate_area `Circle {rad=6.7};
calculate_area `Rectangle {height=1.1; length=2.2};
```

Notice that within the Circle branch, the code can access the rad field, and within the Rectangle branch, it can access the length and height field. Variants and matches let you essentially "unmix" distinct data types after they are mixed together in the program flow. Without variants, this would be impossible to do in a type-safe manner.


## Wildcard matches

The above match expressions are *exclusive*, meaning that any unhandled variant results in a compile time error. You can optionally instead include a *wildcard* case, which will match any variants not matched by the previous match cases:


```ocaml
let calculate_area = fun shape ->
    match shape with
        | `Circle {rad} -> rad *. rad *. 3.1415926
        | `Rectangle {length; height} -> length *. height
        |  v -> "got something unexpected!"
```

Within a wildcard match, the bound variable has the same type as the input expression, except with the previously matched cases statically excluded. For example, in the `calculate_area` example above, `v` would have the type "same as `shape` except not a `Circle` or `Rectangle`".

This makes it possible to further match on the wildcard value elsewhere. For example, in the below code, the new `calculate_area2` function explicitly handles the `Square` case and otherwise defers to the previously defined function to handle the `Circle` and `Rectangle` cases. This works because the compiler knows that the `v` in the wildcard branch is not a `Square`, so it will not complain that the original `calculate_area` function fails to handle squares.

```ocaml
let calculate_area = fun shape ->
    match shape with
        | `Circle {rad} -> rad *. rad *. 3.1415926
        | `Rectangle {length; height} -> length *. height;

let calculate_area2 = fun shape ->
    match shape with
        | `Square {len} -> len *. len
        |  v -> calculate_area v;

calculate_area2 `Circle {rad=6.7};
calculate_area2 `Square {len=9.17};
```

## Advanced pattern matching

Match expressions aren't limited to matching on a single variant. The match cases can be arbitrary patterns, including record and tuple patterns. In this example, `x` is expected to be a tuple, and we use tuple patterns with nested variant patterns to match on two different tags at once:

```ml
let f = fun x -> match x with 
| (`A, `A) -> 0
| (`A, `B) -> 2
| (`B, `A) -> 3
| (`B, `B) -> 4
;
print f (`A(), `B()); // 2
```

Match cases do not have to be exclusive. In the case where multiple match cases could match, the first matching case is chosen:

```ml
let f = fun x -> match x with 
| (`A, _) -> 0
| (_, `B) -> 1
| (_, _) -> 2
;
print f (`B(), `B()); // 1
```

In this example, both the second and third cases match, but the second one is chosen since it comes first. 

However, it is a compile error if a match case is the same or strictly more specific than another match case that appears above it, since this means the latter case can never possibly be chosen:


```ml
let f = fun x -> match x with
| `A -> 0
| `B -> 1
| `A -> 2
;
```
```
SyntaxError: Match case is unreachable.
| `A -> 0
| `B -> 1
| `A -> 2
  ^~      
;
Note: All values are covered by previous match case here:
let f = fun x -> match x with
| `A -> 0
  ^~      
| `B -> 1
| `A -> 2
```


## Match guard expressions


Match cases can have an optional *guard expression*, written as `when <expr>` after the pattern part. If the pattern part of the case matches, then the guard expression is evaluated (including any bindings from that pattern). If the guard expression evaluates to `true`, that case is chosen. Otherwise, matching continues with subsequent match cases:

```ml
let f = fun x -> match x with 
| `A y when y == 2 -> "Hello"
| `A    -> "World"
;
print f `A 2; // Hello
print f `A 9.2; // World
```


It is possible for guard expressions to mutate the very value which is being matched upon. In this case pattern matching is done on the *initial* value and does not see changes that occur during evaluation of guard expressions.

In this example, the guard expression changes `r.x` to have tag `B`, but the third arm is taken rather than the second, because the tag used for pattern matching is still the initial tag `A`.

```ml
let r = {mut x=`A 0};

let res = match r with 
| {x=`A} when (r.x <- `B 0; false) -> 1
| {x=`B}                           -> 2
| {x=`A}                           -> 3
;

print (res, r);  // (3, {x=B 0})
```

## Multi-case match arms

You can leave out the `-> <expr>`, in which case it will fall through to the next match case that has a right hand side expression. This allows multiple match cases to share the same match arm code:

```ml
let f = fun x -> match x with 
| `A 
| `B -> "A or B"
| `C -> "C"
;

print f `A 0; // "A or B"
print f `B 0; // "A or B"
print f `C 0; // "C"
```

Each match case can have its own guard expression, even when sharing the same arm code:

```ml
let f = fun x -> match x with 
| `A y when y == 2
| `B -> "A 2 or B"
| `A
| `C -> "A _ or C"
;

print f `A 2; // "A 2 or B"
print f `A 0; // "A _ or C"
```

## As patterns

Record patterns may optionally be followed by `as` and a variable pattern. This lets you match/destructure a record *and* bind the entire value to a variable at the same time:

```ml
let record = {a=3; b=4; c={d=5; e=6; f={a=90; k=-1}}};
let {a; b=x; c={d; e=y; f={a=z} as f} as c} = record;

print a, x, d, y, z; // 3 4 5 6 90
print c;             // {d=5; e=6; f={a=90; k=-1}}
print f;             // {a=90; k=-1}
```

As patterns are not allowed in function argument patterns.

## Loop expressions

1SubML supports looping via the `loop` expression. This takes the form `loop <body>`, where `<body>` is an expression that must evaluate to a value of type ``[`Break t | `Continue any]`` for some type `t`. The body expression is evaluated repeatedly until it returns `Break`, and the wrapped value becomes the value of the loop expression as a whole.

For example, ``loop `Break 42`` is equivalent to just `42`, while ``loop `Continue ()`` is an infinite loop. In order to do something more interesting, you need to use mutable state, which means a record with [mutable fields](#mutable-fields). Here's an imperative fibonacci calculator using a loop and mutable counter:

```ocaml
let fib = fun n -> (
    let r = {mut n; mut a=1; mut b=1}; // record to hold mutable state
    loop if r.n <= 1 then
            `Break r.a
        else (
            r.n <- r.n - 1;
            let old_a = r.a;
            r.a <- r.a + r.b;
            r.b <-  old_a;
            `Continue ()
        ) 
);
print "fib 1000 =", fib 1000;
```




## Short circuit boolean operators

The `&&` and `||` operators operate on booleans, returning the logical "and" and "or" of the operands respectively. However, they have *short circuit* behavior, meaning that evaluation stops as soon as a false value (for `&&`) or true value (for `||`) is seen. In this example, the "print 3" is never executed due to this short circuiting behavior:

```ml
let t = true;
let f = false;

print (print 1; t) && (print 2; f) && (print 3; t); // prints 1 2 false
print (print 4; f) || (print 5; f) || (print 6; t); // prints 4 5 6 true
```

---

Next: [Chapter 2: Types](guide_ch2.md)
