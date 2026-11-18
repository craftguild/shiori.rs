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
        body: JSON.stringify({ query: '{post(id:"1"){document{nodes{__typename ... on TextNode{id text} ... on ImageNode{altIds destination title presentation{url candidates{url width} sizes width height}}}}}}' }),
      });
      const result = await response.json();
      if (result.errors) throw new Error(JSON.stringify(result.errors));
      const main = document.querySelector('main')!;
      if (override) main.style.maxWidth = '300px';
      // This fixture is a consumer of typed delivery data; article markup belongs to the client.
      const nodes = result.data.post.document.nodes;
      for (const node of nodes) {
        if (node.__typename !== 'ImageNode') continue;
        const value = node.presentation;
        const figure = document.createElement('figure');
        const image = document.createElement('img');
        image.alt = node.altIds.map((id: string) => nodes.find((item: { id?: string }) => item.id === id).text).join('');
        image.style.cssText = 'width:auto;height:auto;max-width:100%';
        if (value.width && value.height) {
          image.width = value.width;
          image.height = value.height;
          const authored = new URL(node.destination, cms);
          const boundWidth = Number(authored.searchParams.get('width')) || value.width;
          const boundHeight = Number(authored.searchParams.get('height')) || value.height;
          const displayWidth = Math.min(value.width, boundWidth, boundHeight * value.width / value.height);
          image.style.maxWidth = `min(100%, ${displayWidth}px)`;
        }
        if (value.candidates.length) {
          image.srcset = value.candidates.map((candidate: { url: string; width: number }) => `${candidate.url.replaceAll(',', '%2C')} ${candidate.width}w`).join(', ');
          image.sizes = override ? '300px' : value.sizes;
        }
        image.src = value.url;
        figure.append(image);
        if (node.title) {
          const caption = document.createElement('figcaption');
          caption.textContent = node.title;
          figure.append(caption);
        }
        main.append(figure);
      }
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
