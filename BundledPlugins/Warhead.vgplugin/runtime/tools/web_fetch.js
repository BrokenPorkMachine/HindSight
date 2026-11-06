import { ArgError, optInt, reqString } from './args.js';
const DEFAULT_MAX_CHARS = 15_000;
const ABSOLUTE_MAX_CHARS = 50_000;
const MIN_CHARS = 500;
function decodeEntities(s) {
    return s
        .replace(/&amp;/g, '&')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
        .replace(/&apos;/g, "'")
        .replace(/&nbsp;/g, ' ')
        .replace(/&#(\d+);/g, (_, dec) => {
        try {
            return String.fromCodePoint(Number(dec));
        }
        catch {
            return _;
        }
    })
        .replace(/&#x([0-9a-f]+);/gi, (_, hex) => {
        try {
            return String.fromCodePoint(Number.parseInt(hex, 16));
        }
        catch {
            return _;
        }
    });
}
function stripTags(s) {
    return s.replace(/<[^>]*>/g, '');
}
export function htmlToMarkdown(html) {
    let text = html
        .replace(/<!--[\s\S]*?-->/g, '')
        .replace(/<script[\s\S]*?<\/script>/gi, '')
        .replace(/<style[\s\S]*?<\/style>/gi, '')
        .replace(/<noscript[\s\S]*?<\/noscript>/gi, '')
        .replace(/<svg[\s\S]*?<\/svg>/gi, '');
    let title = '';
    const titleMatch = /<title[^>]*>([\s\S]*?)<\/title>/i.exec(text);
    if (titleMatch?.[1]) {
        title = stripTags(decodeEntities(titleMatch[1])).trim();
    }
    // Extract from main content container if present
    const mainMatch = /<main[^>]*>([\s\S]*?)<\/main>/i.exec(text) ??
        /<article[^>]*>([\s\S]*?)<\/article>/i.exec(text) ??
        /<body[^>]*>([\s\S]*?)<\/body>/i.exec(text);
    if (mainMatch?.[1]) {
        text = mainMatch[1];
    }
    // Preformatted code blocks
    text = text.replace(/<pre[^>]*>\s*<code[^>]*>([\s\S]*?)<\/code>\s*<\/pre>/gi, (_, code) => {
        return `\n\`\`\`\n${decodeEntities(code).trim()}\n\`\`\`\n`;
    });
    text = text.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, code) => {
        return `\n\`\`\`\n${decodeEntities(code).trim()}\n\`\`\`\n`;
    });
    // Inline code
    text = text.replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, (_, code) => {
        return ` \`${decodeEntities(code).trim()}\` `;
    });
    // Headings
    text = text.replace(/<h1[^>]*>([\s\S]*?)<\/h1>/gi, (_, h) => `\n\n# ${stripTags(decodeEntities(h)).trim()}\n\n`);
    text = text.replace(/<h2[^>]*>([\s\S]*?)<\/h2>/gi, (_, h) => `\n\n## ${stripTags(decodeEntities(h)).trim()}\n\n`);
    text = text.replace(/<h3[^>]*>([\s\S]*?)<\/h3>/gi, (_, h) => `\n\n### ${stripTags(decodeEntities(h)).trim()}\n\n`);
    text = text.replace(/<h[4-6][^>]*>([\s\S]*?)<\/h[4-6]>/gi, (_, h) => `\n\n#### ${stripTags(decodeEntities(h)).trim()}\n\n`);
    // Links
    text = text.replace(/<a[^>]+href="([^">]+)"[^>]*>([\s\S]*?)<\/a>/gi, (_, href, anchor) => {
        const label = stripTags(decodeEntities(anchor)).trim();
        if (!label)
            return '';
        if (href.startsWith('javascript:') || href.startsWith('#'))
            return label;
        return `[${label}](${href})`;
    });
    // List items
    text = text.replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, item) => `\n* ${stripTags(decodeEntities(item)).trim()}`);
    // Structural line breaks
    text = text.replace(/<p[^>]*>/gi, '\n\n');
    text = text.replace(/<\/p>/gi, '\n');
    text = text.replace(/<br\s*\/?>/gi, '\n');
    text = text.replace(/<hr\s*\/?>/gi, '\n---\n');
    text = text.replace(/<\/(div|tr|table|section|article)>/gi, '\n');
    // Strip any remaining markup
    text = stripTags(text);
    text = decodeEntities(text);
    // Normalize spacing
    text = text
        .replace(/[ \t]+/g, ' ')
        .replace(/^[ \t]+|[ \t]+$/gm, '')
        .replace(/\n{3,}/g, '\n\n')
        .trim();
    if (title && !text.toLowerCase().startsWith(`# ${title.toLowerCase()}`)) {
        return `# ${title}\n\n${text}`;
    }
    return text;
}
export const fetchWebPage = {
    mutates: false,
    def: {
        name: 'fetch_web_page',
        description: 'Fetch an HTTP or HTTPS URL and extract its text content as clean readable markdown. Useful for reading documentation, technical specifications, GitHub issues/PRs, and API references found via web_search.',
        parameters: {
            type: 'object',
            properties: {
                url: { type: 'string', description: 'Complete HTTP or HTTPS URL to fetch' },
                max_chars: {
                    type: 'integer',
                    description: `Maximum content characters to return (default ${DEFAULT_MAX_CHARS}, max ${ABSOLUTE_MAX_CHARS})`,
                },
            },
            required: ['url'],
        },
    },
    async run(args) {
        try {
            const rawUrl = reqString(args, 'url').trim();
            let parsed;
            try {
                parsed = new URL(rawUrl);
            }
            catch {
                return { ok: false, output: '', error: `invalid URL: ${rawUrl}` };
            }
            if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
                return { ok: false, output: '', error: `unsupported protocol '${parsed.protocol}'; only http: and https: are allowed` };
            }
            const maxChars = Math.min(ABSOLUTE_MAX_CHARS, Math.max(MIN_CHARS, optInt(args, 'max_chars', MIN_CHARS) ?? DEFAULT_MAX_CHARS));
            const response = await fetch(parsed.toString(), {
                headers: {
                    'user-agent': 'Mozilla/5.0 (compatible; WarheadBot/2.4; +https://github.com/failbr34k/warhead)',
                    accept: 'text/html,application/xhtml+xml,application/json,text/plain,text/markdown;q=0.9,*/*;q=0.8',
                },
                signal: AbortSignal.timeout(20_000),
            });
            if (!response.ok) {
                return { ok: false, output: '', error: `HTTP ${response.status} ${response.statusText}` };
            }
            const contentType = response.headers.get('content-type')?.toLowerCase() ?? '';
            const rawBody = await response.text();
            let content;
            if (contentType.includes('application/json')) {
                try {
                    const obj = JSON.parse(rawBody);
                    content = JSON.stringify(obj, null, 2);
                }
                catch {
                    content = rawBody;
                }
            }
            else if (contentType.includes('text/markdown') || contentType.includes('text/plain')) {
                content = rawBody;
            }
            else {
                // HTML or default
                content = htmlToMarkdown(rawBody);
            }
            if (!content.trim()) {
                return { ok: true, output: `Fetched ${parsed.toString()}: (page was empty or contained no readable text)` };
            }
            const totalChars = content.length;
            if (totalChars > maxChars) {
                content = content.slice(0, maxChars) + `\n\n… [content truncated: showing ${maxChars} of ${totalChars} characters]`;
            }
            return {
                ok: true,
                output: `Content from ${parsed.toString()}:\n\n${content}`,
            };
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
    },
};
