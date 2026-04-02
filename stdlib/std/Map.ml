import Std{map; vec};

let empty = js!%() => new Map%;
let has = js!%a => a._0.has(a._1)%;
let delete = js!%a => a._0.delete(a._1)%;
let set = js!%a => a._0.set(a._1, a._2)%;
let size = js!%a => BigInt(a.size)%;
let clear = js!%a => a.clear()%;
let get = js!%a => a._0.has(a._1) ? {_: 'Some', $: a._0.get(a._1)} : {_: 'None'}%;

let values = js!%a => [...a.values()]%;
let keys = js!%a => [...a.keys()]%;
let items = js!%a => [...a].map(([a, b]) => ({_0: a, _1: b}))%;

export {
    alias t: map;

    empty: [K;V]. any -> map[K; V];
    has: [K]. (map[any<-K; any<-never], K) -> bool;
    set: [K; V]. (map[any<-K; any<-V], K, V) -> any;
    get: [K; V]. (map[any<-K; V<-never], K) -> [`Some V | `None];
    delete: [K]. (map[K; any<-never], K) -> any;

    size: map[any<-never; any<-never] -> int;
    clear: map[any<-never; any<-never] -> any;

    values: [V]. map[any<-never; V<-never] -> vec[V];
    keys: [K]. map[K<-never; any<-never] -> vec[K];
    items: [K; V]. map[K<-never; V<-never] -> vec[(K, V)];
}