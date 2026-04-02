import {
    EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightSpecialChars,
    drawSelection, dropCursor, rectangularSelection, crosshairCursor, highlightActiveLine,
    EditorState,
    oneDark,
    foldGutter, indentOnInput, syntaxHighlighting, defaultHighlightStyle, bracketMatching, foldKeymap,
    history, defaultKeymap, historyKeymap,
    highlightSelectionMatches, searchKeymap,
    autocompletion, completionKeymap,
    lintKeymap,
} from './vendor/codemirror.js';
import {onesubml} from './onesubml.js';
import {STDLIB_FILES} from './stdlib_files.js';

let mod = null;
const mod_promise = import('./pkg/wasm.js').then(
    m => (mod = m, mod.default()));

const EXAMPLE_CODE = `\
(* calculate fibonacci numbers recursively *)
let fib = (
    let rec fib_sub = fun {n; a; b} ->
        if n <= 1 then
            a
        else
            fib_sub {n=n - 1; a=a + b; b=a}
    ;
    fun n -> fib_sub {n; a=1; b=1}
);
(* ints are arbitrary precision *)
(* 999th fibonacci number = 43466557686937456435688527675040625802564660517371780402481729089536555417949051890403879840079255169295922593080322634775209689623239873322471161642996440906533187938298969649928516003704476137795166849228875 *)
print "fib 999 =", fib 999;

(* calculate with explicit loop instead *)
let fib = fun n -> (
    let r = {mut n; mut a=1; mut b=1}; // record to hold mutable state
    loop if r.n <= 1 then
            \`Break r.a
        else (
            r.n <- r.n - 1;
            let old_a = r.a;
            r.a <- r.a + r.b;
            r.b <-  old_a;
            \`Continue ()
        ) 
);
print "fib 1000 =", fib 1000;

(* matching on variant types *)
let area = fun shape ->
    match shape with
    | \`Circle {rad} -> rad *. rad *. 3.1415926
    | \`Rect {length; height} -> length *. height
    ;

print "area \`Circle {rad=5.0} =", area \`Circle {rad=5.0};
print "area \`Rect {height=4.; length=2.5} =", area \`Rect {height=4.; length=2.5};

(* wildcard match delegates to first area function
    for the non-Circle cases in a type safe manner *)
let area = fun shape ->
    match shape with
    | \`Square {len} -> len *. len
    |  v -> area v
    ;

print "area \`Square {len=4.} =", area \`Square {len=4.};
print "area \`Rect {height=4.; length=2.5} =", area \`Rect {height=4.; length=2.5};
print "area \`Circle {rad=1.2} =", area \`Circle {rad=1.2};

// Simulate GADTs using explicit equality witnesses and wrapper functions.
type rec ast[+T] = 
    | Val T 
    | Eq (bool => T, ast[any], ast[any]) 
    | Plus (int => T, ast[int], ast[int]) 
    | If (ast[bool], ast[T], ast[T])
    ;
let Eq = fun (lhs: ast[any], rhs: ast[any]) :: ast[bool] -> Eq (id!, lhs, rhs);
let Plus = fun (lhs: ast[int], rhs: ast[int]) :: ast[int] -> Plus (id!, lhs, rhs);

// Polymorphic recursive evaluation function for the ast "GADT".
let rec eval = fun[T] expr: ast[T] :: T -> 
    match expr with 
    | \`Val x -> x
    | \`Eq (T, lhs, rhs) -> T (eval lhs == eval rhs)
    | \`Plus (T, lhs, rhs) -> T (eval lhs + eval rhs)
    | \`If (cond, lhs, rhs) -> (if eval cond then eval lhs else eval rhs)
    ;

print eval (Plus (Plus (Val 11, Val 2), Val 53));
print eval (Eq (Plus (Val 11, Val 2), Val 13));
print eval (If (Eq (Val true, Val false), Val "equal", Val "not equal"));


`;

// --- File system ---
const files = new Map();
let activeFile = 'main';
files.set('main', EXAMPLE_CODE);

// --- CodeMirror setup ---
function createEditor(shadowRoot, parent, initialContent) {
    return new EditorView({
        root: shadowRoot,
        parent,
        state: EditorState.create({
            doc: initialContent,
            extensions: [
                lineNumbers(),
                highlightActiveLineGutter(),
                highlightSpecialChars(),
                history(),
                foldGutter(),
                drawSelection(),
                dropCursor(),
                EditorState.allowMultipleSelections.of(true),
                indentOnInput(),
                syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
                bracketMatching(),
                autocompletion(),
                rectangularSelection(),
                crosshairCursor(),
                highlightActiveLine(),
                highlightSelectionMatches(),
                keymap.of([
                    ...defaultKeymap,
                    ...searchKeymap,
                    ...historyKeymap,
                    ...foldKeymap,
                    ...completionKeymap,
                    ...lintKeymap,
                ]),
                oneDark,
                onesubml(),
                EditorView.theme({
                    '&': { height: '100%' },
                    '.cm-scroller': { overflow: 'auto' },
                }),
            ],
        }),
    });
}

function setEditorContent(view, content) {
    view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: content },
    });
}

function getEditorContent(view) {
    return view.state.doc.toString();
}

// --- Tab management ---
function renderTabs(root, view) {
    const tabBar = root.getElementById('tab-bar');
    const addBtn = root.getElementById('new-tab');

    // Remove existing tabs (keep the + button)
    tabBar.querySelectorAll('.tab').forEach(t => t.remove());

    for (const name of files.keys()) {
        const tab = document.createElement('div');
        tab.className = 'tab' + (name === activeFile ? ' active' : '');
        tab.dataset.name = name;

        const label = document.createElement('span');
        label.className = 'tab-label';
        label.textContent = name;
        tab.appendChild(label);

        if (name !== 'main') {
            const close = document.createElement('span');
            close.className = 'close';
            close.textContent = '\u00d7';
            close.addEventListener('click', e => {
                e.stopPropagation();
                deleteFile(root, view, name);
            });
            tab.appendChild(close);
        }

        tab.addEventListener('click', () => switchTab(root, view, name));

        // Double-click to rename (not main)
        if (name !== 'main') {
            tab.addEventListener('dblclick', e => {
                e.preventDefault();
                startRename(root, view, tab, name);
            });
        }

        tabBar.insertBefore(tab, addBtn);
    }
}

function switchTab(root, view, name) {
    if (name === activeFile) return;
    // Save current content
    files.set(activeFile, getEditorContent(view));
    activeFile = name;
    setEditorContent(view, files.get(name) || '');
    renderTabs(root, view);
    view.focus();
}

function createNewFile(root, view) {
    // Find a unique name
    let n = 1;
    while (files.has('Module' + n)) n++;
    const name = 'Module' + n;
    files.set(name, '');
    switchTab(root, view, name);
}

function deleteFile(root, view, name) {
    if (name === 'main') return;
    files.delete(name);
    if (activeFile === name) {
        activeFile = 'main';
        setEditorContent(view, files.get('main') || '');
    }
    renderTabs(root, view);
}

function startRename(root, view, tabEl, oldName) {
    const label = tabEl.querySelector('.tab-label');
    const input = document.createElement('input');
    input.className = 'tab-rename-input';
    input.value = oldName;
    label.replaceWith(input);
    input.focus();
    input.select();

    function finishRename() {
        const newName = input.value.trim();
        if (newName && newName !== oldName && !files.has(newName) && newName !== 'main') {
            const content = files.get(oldName);
            files.delete(oldName);
            files.set(newName, content);
            if (activeFile === oldName) activeFile = newName;
        }
        renderTabs(root, view);
    }

    input.addEventListener('blur', finishRename);
    input.addEventListener('keydown', e => {
        if (e.key === 'Enter') { e.preventDefault(); input.blur(); }
        if (e.key === 'Escape') { input.value = oldName; input.blur(); }
    });
}

// --- Sync files to compiler ---
function syncFilesToCompiler(compiler) {
    compiler.clear_files();
    // Register stdlib files first (user files can shadow them)
    for (const [name, content] of Object.entries(STDLIB_FILES)) {
        compiler.set_file(name, content);
    }
    for (const [name, content] of files) {
        if (name !== 'main') {
            compiler.set_file(name, content);
        }
    }
}

// --- REPL logic (preserved from original) ---
function initializeRepl(root, compiler, view) {
    const container = root.getElementById('container');
    const output = root.getElementById('output');
    const prompt = root.getElementById('prompt');

    function addOutput(line, cls) {
        const l = document.createElement('pre');
        l.textContent = line;
        if (cls) l.classList.add(cls);
        output.appendChild(l);
        return l;
    }

    const $ = Object.create(null);
    const history = [];
    let history_offset = -1;

    function execCode(script) {
        let compiled;
        try {
            if (!compiler.process(script)) { return [false, compiler.get_err()]; }
            compiled = '(' + compiler.get_output() + ')';
        } catch (e) {
            return [false, 'Internal compiler error: ' + e.toString() +
                '\nIf you see this message, please file an issue on Github with the code required to trigger this error.'];
        }

        try {
            const p = new Printer;
            const val = eval(compiled);
            if (val !== undefined) p.visitRoot(val);
            return [true, p.parts.join('')];
        } catch (e) {
            return [false, 'An error occurred during evaluation in the repl: ' + e.toString()];
        }
    }

    function processCode(script) {
        const [success, res] = execCode(script);
        addOutput(res, success ? 'success' : 'error');
        output.scrollTop = output.scrollHeight;
        return success;
    }

    function processReplInput(line) {
        line = line.trim();
        if (!line) return;

        history_offset = -1;
        if (history[history.length - 1] !== line) history.push(line);
        addOutput('>>\u00a0' + line, 'input');
        processCode(line);
    }

    root.getElementById('compile-and-run').addEventListener('click', () => {
        // Save current editor content
        files.set(activeFile, getEditorContent(view));
        const mainContent = files.get('main').trim();
        if (!mainContent) return;

        output.textContent = '';
        compiler.reset();
        syncFilesToCompiler(compiler);
        if (processCode(mainContent)) prompt.focus({ preventScroll: true });
    });

    prompt.addEventListener('keydown', e => {
        switch (e.key) {
            case 'ArrowDown': history_offset -= 1; break;
            case 'ArrowUp': history_offset += 1; break;
            default: return;
        }
        e.preventDefault();
        if (history_offset >= history.length) history_offset = history.length - 1;
        if (history_offset < 0) history_offset = 0;
        prompt.value = history[history.length - history_offset - 1];
    });

    root.getElementById('space-below-prompt').addEventListener('click', e => {
        e.preventDefault();
        prompt.focus({ preventScroll: true });
    });

    root.getElementById('rhs-form').addEventListener('submit', e => {
        e.preventDefault();
        const s = prompt.value.trim();
        prompt.value = '';
        if (!s) return;
        processReplInput(s);
    });

    container.classList.remove('loading');
    prompt.disabled = false;
    container.removeChild(root.getElementById('loading'));

    // Run example code on load
    syncFilesToCompiler(compiler);
    processCode(files.get('main').trim());
}

// --- Web Component ---
class OnesubmlDemo extends HTMLElement {
    constructor() {
        super();
        const shadow = this.attachInternals().shadowRoot;

        mod_promise.then(wasm => {
            const compiler = mod.State.new();
            const editorContainer = shadow.getElementById('editor-container');
            const view = createEditor(shadow, editorContainer, files.get('main'));

            // Set up tabs
            renderTabs(shadow, view);
            shadow.getElementById('new-tab').addEventListener('click', () => {
                createNewFile(shadow, view);
            });

            initializeRepl(shadow, compiler, view);
        }).catch(e => {
            shadow.getElementById('loading').textContent = 'Failed to load demo: ' + e;
        });
    }
}
customElements.define('onesubml-demo', OnesubmlDemo);
