import Std{dyn};

let new = js!%() => {let s = Symbol(); let wrap=t => [s, t]; let unwrap=([a, b]) => (a === s) ? {_:'Some', $: b} : {_:'None'}; return {wrap, unwrap}}%;

export {
    alias t: dyn;

    new: [T]. any -> {wrap: T -> dyn; unwrap: dyn -> [`Some T | `None]};
}