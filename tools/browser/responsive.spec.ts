import { test, expect } from '@playwright/test';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

let server: Server;
let origin: string;
const cms = process.env.SHIORI_ACCEPTANCE_CMS;

test.beforeAll(async () => {
  if (!cms) throw new Error('Run through the Rust release-binary acceptance fixture');
  server = createServer((_request, response) => {
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end('<!doctype html><meta charset="utf-8"><style>body{margin:16px}main{width:calc(100vw - 48px);max-width:700px}figure{margin:16px 0}</style><main></main>');
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

test.afterAll(async () => {
  await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
});

for (const [width, override] of [[400, false], [1100, false], [1100, true]] as const) {
  test(`decode and size actual images at ${width}px, override=${override}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(origin);
    const result = await page.evaluate(async ({ cms, override }) => {
      const response = await fetch(cms + '/graphql', {
        method: 'POST', headers: { 'content-type': 'application/json', 'x-chigiri-preflight': '1' },
        body: JSON.stringify({ query: '{ post(id:"1") { blocks { html } } }' }),
      });
      const result = await response.json();
      if (result.errors) throw new Error(JSON.stringify(result.errors));
      const template = document.createElement('template');
      template.innerHTML = result.data.post.blocks.map((block: { html: string }) => block.html).join('');
      const main = document.querySelector('main')!;
      if (override) {
        main.style.maxWidth = '300px';
        for (const image of template.content.querySelectorAll<HTMLImageElement>('img[srcset]')) image.sizes = '300px';
      }
      main.append(template.content);
      const rows = [];
      for (const image of document.querySelectorAll('img')) {
        await image.decode();
        const current = image.currentSrc;
        const download = await fetch(current);
        if (!download.ok) throw new Error('image HTTP ' + download.status);
        const value = image.getAttribute('srcset') || '';
        const blob = await download.blob();
        const bitmap = value ? await createImageBitmap(blob) : { width: image.naturalWidth, height: image.naturalHeight, close() {} };
        const declared = [];
        const expression = /\s*(\S+)\s+([1-9][0-9]*)w(?:\s*,\s*|$)/gy;
        let match;
        while ((match = expression.exec(value))) declared.push({ url: match[1], width: Number(match[2]) });
        const selected = declared.find(candidate => new URL(candidate.url, document.baseURI).href === current);
        const expected = declared.find(candidate => candidate.width >= (override ? 300 : innerWidth)) || declared.at(-1);
        const rect = image.getBoundingClientRect();
        const parent = main.getBoundingClientRect();
        const intrinsicWidth = Number(image.getAttribute('width')) || bitmap.width;
        const intrinsicHeight = Number(image.getAttribute('height')) || bitmap.height;
        const expectedHeight = selected ? Math.max(1, Math.floor((intrinsicHeight * selected.width + Math.floor(intrinsicWidth / 2)) / intrinsicWidth)) : bitmap.height;
        const noUpscale = rect.width <= intrinsicWidth + 0.5 && rect.height <= intrinsicHeight + 0.5;
        const aspectError = Math.abs(rect.height - rect.width * intrinsicHeight / intrinsicWidth);
        const explicitHeight = image.alt !== 'Height bound' || (Math.abs(rect.width - 40) <= 0.5 && Math.abs(rect.height - 20) <= 0.5);
        const ok = image.complete && rect.width > 0 && rect.width <= parent.width + 1 && noUpscale && aspectError <= 1 && explicitHeight &&
          (!value || (selected && selected.width === bitmap.width && expected!.width === selected.width && bitmap.height === expectedHeight));
        rows.push({ alt: image.alt, src: image.src, current, width: bitmap.width, height: bitmap.height,
          displayWidth: rect.width, displayHeight: rect.height, noUpscale, aspectError, explicitHeight, ok });
        bitmap.close();
      }
      return { rows, captions: [...document.querySelectorAll('figcaption')].map(caption => caption.textContent) };
    }, { cms: cms!, override });
    expect(result.rows).toHaveLength(15);
    for (const row of result.rows) expect(row.ok, JSON.stringify(row)).toBeTruthy();
    expect(result.captions).toContain('Wide caption');
    expect(result.captions).toContain('Alpha caption');
  });
}
