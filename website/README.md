# Shale product site

Static marketing site for [Shale](https://github.com/Alevetto07/Shale). Built with [Astro](https://astro.build) and a small React island for the ASCII Andromeda hero.

## Prerequisites

- Node.js 20+

## Develop

```bash
cd website
npm install
npm run dev
```

Open the URL Astro prints (default `http://localhost:4321`).

## Build

```bash
npm run build
```

Output lands in `dist/` — plain HTML, CSS and JS suitable for any static host.

## Deploy on Vercel

1. Import the repository in Vercel (or add this directory as a monorepo project).
2. Set **Root Directory** to `website`.
3. **Framework preset**: Astro.
4. **Build command**: `npm run build`
5. **Output directory**: `dist`
6. No environment variables are required for the static site.

Preview deployments work out of the box; point a custom domain at the production deployment when you are ready.

## Release copy and downloads

Edit `src/config/site.ts` when you tag a release: version string, file names, SHA-256 hashes and sizes for the three GitHub Release assets.

## Screenshots of the site

```bash
npm run build
npm run preview &   # serves dist on :4321 by default
npm run screenshots # writes to ../artifacts after preview is up
```

The capture script expects the preview server on `http://127.0.0.1:4321`.