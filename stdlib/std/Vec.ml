import Std{vec};

let empty = js!%() => []%;
let size = js!%a => BigInt(a.length)%;
let clear = js!%a => a.length = 0%;
let truncate = js!%({_0:v, _1:k}) => {let i=Number(k); if (i >= 0 && i < v.length) {v.length = i}}%;

let get = js!%({_0:v, _1:k}) => {let i=Number(k); return (i >= 0 && i < v.length) ? {_:'Some', $:v[i]} : {_:'None'}}%;
let set = js!%({_0:v, _1:k, _2:val}) => {let i=Number(k); if (i >= 0 && i < v.length) {v[i] = val}}%;
let push = js!%a => a._0.push(a._1)%;
let pop = js!%a => a.length ? {_: 'Some', $: a.pop()} : {_: 'None'}%;

let map = js!%a => a._0.map(a._1)%;
let filter = js!%a => a._0.filter(a._1)%;

export {
    alias t: vec;

    empty: [T]. any -> vec[T];
    size: vec[any<-never] -> int;
    clear: vec[any<-never] -> any;
    truncate: (vec[any<-never], int) -> any;


    get: [T]. (vec[T<-never], int) -> [`Some T | `None];
    set: [T]. (vec[any<-T], int, T) -> any;
    push: [T]. (vec[any<-T], T) -> any;
    pop: [T]. vec[T<-never] -> [`Some T | `None];

    map: [T; U]. (vec[T<-never], T -> U) -> vec[U];
    filter: [T]. (vec[T<-never], T -> bool) -> vec[T];
}