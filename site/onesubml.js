import {StreamLanguage} from './vendor/codemirror.js';

const keywords = new Set([
    'let', 'rec', 'fun', 'if', 'then', 'else', 'match', 'with', 'when',
    'type', 'alias', 'loop', 'mut', 'and', 'as', 'mod', 'import', 'export',
    'begin', 'end', 'implicit', 'print',
]);

const builtins = new Set(['true', 'false']);

const parser = {
    name: 'onesubml',

    startState() {
        return { inBlockComment: false };
    },

    token(stream, state) {
        // Continue block comment from previous line
        if (state.inBlockComment) {
            while (!stream.eol()) {
                if (stream.match('*)')) {
                    state.inBlockComment = false;
                    return 'blockComment';
                }
                stream.next();
            }
            return 'blockComment';
        }

        // Skip whitespace
        if (stream.eatSpace()) return null;

        // Block comment start
        if (stream.match('(*')) {
            while (!stream.eol()) {
                if (stream.match('*)')) return 'blockComment';
                stream.next();
            }
            state.inBlockComment = true;
            return 'blockComment';
        }

        // Line comment
        if (stream.match('//')) {
            stream.skipToEnd();
            return 'lineComment';
        }

        // String
        if (stream.eat('"')) {
            while (!stream.eol()) {
                const ch = stream.next();
                if (ch === '\\') { stream.next(); continue; }
                if (ch === '"') return 'string';
            }
            return 'string';
        }

        // Tag: backtick followed by identifier
        if (stream.eat('`')) {
            stream.match(/^[A-Za-z_$][\w$]*/);
            return 'tagName';
        }

        // Numbers
        if (stream.match(/^-?[0-9]+\.[0-9]*(?:[eE]-?[0-9]+)?/) ||
            stream.match(/^-?[0-9]+[eE]-?[0-9]+/)) {
            return 'float';
        }
        if (stream.match(/^-?[0-9]+/)) {
            return 'integer';
        }

        // Multi-char operators (order matters - longer first)
        if (stream.match('constructor-of!')) return 'keyword';
        if (stream.match('id!')) return 'keyword';
        if (stream.match('<=.') || stream.match('>=.')) return 'operator';
        if (stream.match('->') || stream.match('=>') || stream.match('<-') ||
            stream.match('|>') || stream.match('&&') || stream.match('||') ||
            stream.match('==') || stream.match('!=') || stream.match('<=') ||
            stream.match('>=') || stream.match('::') || stream.match(':>') ||
            stream.match('+.') || stream.match('-.') || stream.match('*.') ||
            stream.match('/.') || stream.match('%.') || stream.match('<.') ||
            stream.match('>.')) {
            return 'operator';
        }

        // Identifiers and keywords
        if (stream.match(/^[A-Za-z_$][\w$]*/)) {
            const word = stream.current();
            if (keywords.has(word)) return 'keyword';
            if (builtins.has(word)) return 'bool';
            return 'variableName';
        }

        // Single-char operators
        const ch = stream.peek();
        if ('+-*/%<>=|^:.!'.includes(ch)) {
            stream.next();
            return 'operator';
        }

        // Punctuation
        if ('(){}[];,'.includes(ch)) {
            stream.next();
            return 'punctuation';
        }

        // Fallback: consume character
        stream.next();
        return null;
    },
};

export function onesubml() {
    return StreamLanguage.define(parser);
}
