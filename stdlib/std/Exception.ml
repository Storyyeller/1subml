let panic = js!%s => {throw new Error(s)}%;
export {
    panic: str -> never;
}