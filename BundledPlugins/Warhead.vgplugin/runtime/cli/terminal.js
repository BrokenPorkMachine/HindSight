import { stripVTControlCharacters } from 'node:util';
const SGR = /\x1b\[[0-9;:]*m/g;
const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
function cellWidth(cluster) {
    if (/^[\p{Mark}\p{Cf}]*$/u.test(cluster))
        return 0;
    if (/\p{Emoji_Presentation}|\p{Regional_Indicator}|\uFE0F|\u20E3/u.test(cluster) && !cluster.includes('\uFE0E'))
        return 2;
    const code = cluster.codePointAt(0) ?? 0;
    return (code >= 0x1100 && code <= 0x115f) || (code >= 0x2329 && code <= 0x232a) ||
        (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) || (code >= 0xac00 && code <= 0xd7a3) ||
        (code >= 0xf900 && code <= 0xfaff) || (code >= 0xfe10 && code <= 0xfe19) ||
        (code >= 0xfe30 && code <= 0xfe6f) || (code >= 0xff01 && code <= 0xff60) ||
        (code >= 0xffe0 && code <= 0xffe6) || (code >= 0x20000 && code <= 0x3fffd) ? 2 : 1;
}
function plain(value) {
    return stripVTControlCharacters(value).replace(/[\x00-\x1f\x7f-\x9f]/g, '');
}
export function terminalCellWidth(value) {
    let width = 0;
    for (const { segment } of graphemes.segment(plain(value)))
        width += cellWidth(segment);
    return width;
}
/** Keep SGR colors, strip cursor/OSC controls, and never split a grapheme. */
function* tokens(text) {
    let start = 0;
    for (const match of text.matchAll(SGR)) {
        for (const { segment } of graphemes.segment(plain(text.slice(start, match.index)).replace(/\t/g, '    ')))
            yield segment;
        yield match[0];
        start = match.index + match[0].length;
    }
    for (const { segment } of graphemes.segment(plain(text.slice(start))))
        yield segment;
}
function* wrappedRows(text, width) {
    if (width < 1) {
        yield '';
        return;
    }
    let line = '', cells = 0, style = '';
    for (let token of tokens(text.replace(/\t/g, '    '))) {
        if (token.startsWith('\x1b[')) {
            style = token === '\x1b[m' || token === '\x1b[0m' ? '' : style + token;
            line += token;
            continue;
        }
        let size = cellWidth(token);
        if (size > width) {
            token = '�';
            size = 1;
        }
        if (cells + size > width && cells > 0) {
            yield line + (style ? '\x1b[0m' : '');
            line = style;
            cells = 0;
        }
        line += token;
        cells += size;
    }
    yield line + (style ? '\x1b[0m' : '');
}
/** Wrap physical rows ourselves so the terminal cannot scroll the composer. */
export function wrapTerminalLine(text, width) {
    return [...wrappedRows(text, width)];
}
export function clipTerminalLine(text, width) {
    if (width < 1)
        return '';
    const rows = wrappedRows(text, width);
    const first = rows.next().value ?? '';
    if (rows.next().done)
        return first;
    return width === 1 ? '…' : `${wrappedRows(text, width - 1).next().value ?? ''}…`;
}
