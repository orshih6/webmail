import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Pure SPA: the Rust binary embeds build/ and serves index.html for any unknown path.
			adapter: adapter({ fallback: 'index.html' }),
			// SvelteKit hashes its inline bootstrap script into a <meta> CSP; the Rust server adds
			// only what a <meta> policy cannot carry (frame-ancestors).
			csp: {
				mode: 'hash',
				directives: {
					'default-src': ['self'],
					'script-src': ['self'],
					// https: only matters inside the message iframe (srcdoc inherits this policy), and the
					// server strips remote images there unless the user clicked "show images".
					'img-src': ['self', 'data:', 'https:'],
					'style-src': ['self', 'unsafe-inline'],
					'frame-src': ['self'],
					'object-src': ['none'],
					'base-uri': ['none'],
					'form-action': ['self']
				}
			}
		})
	],
	server: {
		// `cargo run` listens on 8080; in dev the browser only ever talks to Vite.
		proxy: { '/api': 'http://localhost:8080' }
	}
});
