import { terminalCellWidth, wrapTerminalLine, clipTerminalLine } from './terminal.js';
import { spawnSync } from 'node:child_process';
import { DIM, BOLD, CYAN, RESET, terminalWidth, terminalHeight, uiRule, attachmentPreview } from './style.js';
/**
 * TTY-only screen layout. The alternate screen keeps normal terminal
 * scrollback intact while giving the active session a real output viewport and
 * a composer anchored below it. Pipe/non-TTY sessions never enter this mode.
 */
export class TerminalLayout {
    rl;
    active = false;
    registeredExit = false;
    lines = [];
    current = '';
    progress = '';
    model = null;
    attachments = [];
    verbose = false;
    contextTokens = 0;
    scrollOffset = 0;
    wrappedWidth = 0;
    wrappedCount = 0;
    wrappedLines = [];
    renderTimer;
    palette = null;
    paletteOffset = 0;
    selecting = false;
    selStart = -1;
    selEnd = -1;
    constructor(rl) {
        this.rl = rl;
    }
    start(model, attachments, verbose, contextTokens = 0) {
        this.active = true;
        this.setComposer(model, attachments, verbose, contextTokens);
        process.stdout.write('\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[H\x1b[?25h');
        if (!this.registeredExit) {
            process.once('exit', () => this.stop());
            this.registeredExit = true;
        }
        this.render();
    }
    stop() {
        if (!this.active)
            return;
        clearTimeout(this.renderTimer);
        this.renderTimer = undefined;
        this.active = false;
        this.palette = null;
        this.selecting = false;
        this.selStart = this.selEnd = -1;
        process.stdout.write('\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[?1049l');
    }
    setPalette(lines) {
        this.palette = lines && lines.length > 0 ? lines : null;
        this.paletteOffset = 0;
        this.selecting = false;
        this.selStart = this.selEnd = -1;
        this.render();
    }
    setComposer(model, attachments, verbose, contextTokens = this.contextTokens) {
        this.model = model;
        this.attachments = attachments;
        this.verbose = verbose;
        this.contextTokens = contextTokens;
    }
    setProgress(text) {
        this.progress = text;
        this.render();
    }
    copyToClipboard(text) {
        try {
            const candidates = process.platform === 'darwin' ? ['pbcopy']
                : process.platform === 'win32' ? ['clip']
                    : ['wl-copy', 'xclip -selection clipboard', 'xsel --clipboard --input'];
            for (const cmd of candidates) {
                const [bin, ...args] = cmd.split(' ');
                const r = spawnSync(bin, args, { input: text });
                if (r.status === 0)
                    break;
            }
        }
        catch { /* clipboard tool unavailable */ }
    }
    snapshot(all = false) {
        const rows = this.outputRows();
        const viewportRows = Math.max(1, terminalHeight() - 5);
        const slice = all ? rows : rows.slice(Math.max(0, rows.length - viewportRows));
        return slice.join('\n');
    }
    handleSequence(sequence) {
        if (!this.active)
            return false;
        if (this.palette && this.palette.length > 0) {
            const maxAvailable = Math.max(1, terminalHeight() - 3);
            const maxVisible = Math.max(1, maxAvailable - 2);
            const total = this.palette.length;
            const mouse = /^\x1b\[<(\d+);(\d+);(\d+)[Mm]$/.exec(sequence);
            if (mouse) {
                const button = Number(mouse[1]);
                if ((button & 64) !== 0) {
                    if ((button & 3) === 0)
                        this.paletteOffset = Math.min(Math.max(0, total - maxVisible), this.paletteOffset + 3);
                    else if ((button & 3) === 1)
                        this.paletteOffset = Math.max(0, this.paletteOffset - 3);
                    this.render();
                }
                return true;
            }
            if (sequence === '\x1b[5~') {
                this.paletteOffset = Math.max(0, this.paletteOffset - maxVisible);
                this.render();
                return true;
            }
            if (sequence === '\x1b[6~') {
                this.paletteOffset = Math.min(Math.max(0, total - maxVisible), this.paletteOffset + maxVisible);
                this.render();
                return true;
            }
            return false;
        }
        const page = Math.max(1, terminalHeight() - 6);
        const mouse = /^\x1b\[<(\d+);(\d+);(\d+)[Mm]$/.exec(sequence);
        if (mouse) {
            const button = Number(mouse[1]);
            const isRelease = sequence.endsWith('m');
            const motion = (button & 32) !== 0;
            const wheel = (button & 64) !== 0;
            const left = (button & 3) === 0;
            const row = Number(mouse[3]) - 1;
            const viewportRows = Math.max(1, terminalHeight() - 5);
            const all = this.outputRows();
            const end = Math.max(0, all.length - this.scrollOffset);
            const startIdx = Math.max(0, end - viewportRows);
            const idx = startIdx + Math.max(0, Math.min(viewportRows - 1, row));
            if (wheel) {
                if (Number(mouse[3]) <= Math.max(1, terminalHeight() - 5)) {
                    if ((button & 3) === 0)
                        this.scrollBy(3);
                    else if ((button & 3) === 1)
                        this.scrollBy(-3);
                }
                return true;
            }
            if (left) {
                if (isRelease) {
                    if (this.selecting) {
                        const lo = Math.min(this.selStart, this.selEnd);
                        const hi = Math.max(this.selStart, this.selEnd);
                        const text = all.slice(lo, hi + 1).join('\n');
                        this.copyToClipboard(text);
                        this.selecting = false;
                        this.selStart = this.selEnd = -1;
                        this.render();
                    }
                }
                else if (motion) {
                    if (this.selecting) {
                        this.selEnd = idx;
                        this.render();
                    }
                }
                else {
                    this.selecting = true;
                    this.selStart = this.selEnd = idx;
                    this.render();
                }
                return true;
            }
            return true;
        }
        if (sequence === '\x1b[5~')
            this.scrollBy(page);
        else if (sequence === '\x1b[6~')
            this.scrollBy(-page);
        else if (sequence === '\x1b[H' || sequence === '\x1b[1~')
            this.scrollToTop();
        else if (sequence === '\x1b[F' || sequence === '\x1b[4~')
            this.scrollToBottom();
        else
            return false;
        return true;
    }
    outputRows() {
        const width = terminalWidth();
        if (width !== this.wrappedWidth) {
            this.wrappedWidth = width;
            this.wrappedCount = 0;
            this.wrappedLines = [];
        }
        while (this.wrappedCount < this.lines.length) {
            const line = this.lines[this.wrappedCount++];
            this.wrappedLines.push(...wrapTerminalLine(line, width));
        }
        return [...this.wrappedLines, ...wrapTerminalLine(this.current, width)];
    }
    scrollBy(delta) {
        const viewportRows = Math.max(1, terminalHeight() - 5);
        const max = Math.max(0, this.outputRows().length - viewportRows);
        this.scrollOffset = Math.max(0, Math.min(max, this.scrollOffset + delta));
        this.render();
    }
    scrollToTop() {
        const viewportRows = Math.max(1, terminalHeight() - 5);
        this.scrollOffset = Math.max(0, this.outputRows().length - viewportRows);
        this.render();
    }
    scrollToBottom() {
        this.scrollOffset = 0;
        this.render();
    }
    write(text) {
        if (!this.active) {
            process.stdout.write(text);
            return;
        }
        const previousRows = this.scrollOffset > 0 ? this.outputRows().length : 0;
        const clean = text
            .replace(/\r/g, '')
            .replace(/\x1b\[\??(?:\d+;?)*[A-HJKSTfhl]/g, '');
        const chunks = clean.split('\n');
        this.current += chunks[0] ?? '';
        for (const chunk of chunks.slice(1)) {
            this.lines.push(this.current);
            this.current = chunk;
        }
        if (this.lines.length > 5000) {
            this.lines.splice(0, this.lines.length - 5000);
            this.wrappedCount = 0;
            this.wrappedLines = [];
        }
        if (this.scrollOffset > 0)
            this.scrollOffset += Math.max(0, this.outputRows().length - previousRows);
        // Coalesce token-sized writes while keeping input/scroll renders immediate.
        if (!this.renderTimer)
            this.renderTimer = setTimeout(() => this.render(), 33);
    }
    render() {
        clearTimeout(this.renderTimer);
        this.renderTimer = undefined;
        if (!this.active)
            return;
        const height = terminalHeight();
        const width = terminalWidth();
        const composerRows = 5;
        const viewportRows = Math.max(1, height - composerRows);
        const allOutput = this.outputRows();
        const maxScroll = Math.max(0, allOutput.length - viewportRows);
        this.scrollOffset = Math.min(this.scrollOffset, maxScroll);
        const end = Math.max(0, allOutput.length - this.scrollOffset);
        const output = allOutput.slice(Math.max(0, end - viewportRows), end);
        const title = ' warhead · composer ';
        const scroll = this.scrollOffset > 0 ? ` · ${DIM}scrolled back · PgDn/End to return${RESET}` : '';
        const status = `${BOLD}${this.model ?? 'no model'}${RESET} ${DIM}· context ~${this.contextTokens} tokens · ${this.verbose ? 'expanded' : 'compact'} tools${RESET}${scroll}`;
        const hint = width < 68 ? 'PgUp/PgDn/wheel scroll · /help · Esc×2 cancel' : 'PgUp/PgDn or wheel scroll · Home/End jump · /help · Esc twice cancels';
        let frame = '\x1b[2J\x1b[H';
        for (let i = 0; i < viewportRows; i++) {
            const idx = Math.max(0, end - viewportRows) + i;
            let line = output[i] ?? '';
            if (this.selecting && this.selStart >= 0 && this.selEnd >= 0) {
                const lo = Math.min(this.selStart, this.selEnd);
                const hi = Math.max(this.selStart, this.selEnd);
                if (idx >= lo && idx <= hi)
                    line = `\x1b[7m${line}\x1b[27m`;
            }
            frame += `${line}\x1b[K\n`;
        }
        frame += `\x1b[${viewportRows + 1};1H${DIM}╭─${BOLD}${CYAN}${title}${RESET}${DIM}${uiRule(width, title)}╮${RESET}\x1b[K\n`;
        frame += `${DIM}│${RESET} ${clipTerminalLine(status, width - 2)}\x1b[K\n`;
        const attachmentLine = attachmentPreview(this.attachments, 2, 24, Math.max(1, width - 4));
        const progressText = this.progress && terminalCellWidth(attachmentLine) + terminalCellWidth(this.progress) + 3 <= width - 4
            ? ` ${DIM}· ${this.progress}${RESET}` : '';
        frame += `${DIM}│${RESET} ${attachmentLine}${progressText}\x1b[K\n`;
        frame += `${DIM}╰─${RESET} ${DIM}${clipTerminalLine(hint, width - 3)}${RESET}\x1b[K`;
        if (this.palette && this.palette.length > 0) {
            const maxAvailable = Math.max(1, height - 3);
            const maxVisible = Math.max(1, maxAvailable - 2);
            const total = this.palette.length;
            if (this.paletteOffset > total - maxVisible)
                this.paletteOffset = Math.max(0, total - maxVisible);
            const count = Math.min(total - this.paletteOffset, maxVisible);
            if (count > 0) {
                const overlayRows = count + 2;
                const startRow = Math.max(1, Math.min(height - overlayRows + 1, Math.floor((height - overlayRows) / 2)));
                const paletteTitle = width > 35 ? ' commands · Tab completes ' : ' commands ';
                frame += `\x1b[${startRow};1H${DIM}╭─${BOLD}${CYAN}${paletteTitle}${RESET}${DIM}${uiRule(width, paletteTitle)}╮${RESET}\x1b[K`;
                for (let i = 0; i < count; i++) {
                    const row = startRow + 1 + i;
                    frame += `\x1b[${row};1H${DIM}│${RESET} ${clipTerminalLine(this.palette[this.paletteOffset + i], width - 3)}\x1b[K`;
                }
                const bottomRow = startRow + 1 + count;
                frame += `\x1b[${bottomRow};1H${DIM}╰${'─'.repeat(Math.max(1, width - 2))}╯${RESET}\x1b[K`;
            }
        }
        process.stdout.write(frame);
        process.stdout.write(`\x1b[${height};1H`);
        this.rl.prompt(true);
    }
}
