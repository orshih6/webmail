<script lang="ts" module>
/** Plain text → editor HTML. Lines starting with "> " become blockquotes. */
export function textToHtml(text: string): string {
	const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
	const out: string[] = [];
	let quote: string[] = [];
	const flush = () => {
		if (quote.length) out.push(`<blockquote>${textToHtml(quote.join('\n'))}</blockquote>`);
		quote = [];
	};
	for (const line of text.split('\n')) {
		if (/^> ?/.test(line)) {
			quote.push(line.replace(/^> ?/, ''));
			continue;
		}
		flush();
		out.push(`${esc(line)}<br>`);
	}
	flush();
	return out.join('');
}
</script>

<script lang="ts">
	import Icon, { type IconName } from './Icon.svelte';

	let {
		html = $bindable(''),
		onimage,
		oninput
	}: {
		html: string;
		/** Uploads a pasted/dropped image and returns its preview URL and id. */
		onimage: (file: File) => Promise<{ id: string; url: string } | null>;
		oninput?: () => void;
	} = $props();

	let el = $state<HTMLDivElement>();
	let uploading = $state(0);

	/** The rendered text, for the plain-text alternative of the message. */
	export function text(): string {
		return el?.innerText.replace(/ /g, ' ').trimEnd() ?? '';
	}

/** Focus with the caret at the very top — above any signature or quoted text. */
export function focus() {
	if (!el) return;
	el.focus();
	const r = document.createRange();
	r.setStart(el, 0);
	r.collapse(true);
	const sel = getSelection();
	sel?.removeAllRanges();
	sel?.addRange(r);
}

	// Push external changes (initial value, signature swap) into the editor; edits made in
	// the editor flow out through `input` and are equal here, so the caret is left alone.
	$effect(() => {
		if (el && el.innerHTML !== html) el.innerHTML = html;
	});

	function sync() {
		if (!el) return;
		html = el.innerHTML;
		oninput?.();
	}

	function cmd(command: string, value?: string) {
		el?.focus();
		document.execCommand(command, false, value);
		sync();
	}

	function link() {
		const url = prompt('Link address (https://…)');
		if (!url) return;
		const u = url.trim();
		if (!/^(https?:|mailto:)/i.test(u)) return alert('Only http(s) and mailto links are allowed.');
		cmd('createLink', u);
	}

	async function insertImages(files: File[]) {
		for (const f of files) {
			uploading++;
			try {
				const up = await onimage(f);
				if (up) {
					el?.focus();
					document.execCommand(
						'insertHTML',
						false,
						`<img src="${up.url}" data-upload="${up.id}" alt="" style="max-width:100%">`
					);
					sync();
				}
			} finally {
				uploading--;
			}
		}
	}

	const imagesOf = (list: DataTransferItemList | FileList | undefined | null) =>
		[...(list ?? [])]
			.map((x) => (x instanceof File ? x : (x as DataTransferItem).getAsFile()))
			.filter((f): f is File => !!f && /^image\/(png|jpeg|gif|webp)$/.test(f.type));

	function onpaste(e: ClipboardEvent) {
		const imgs = imagesOf(e.clipboardData?.items);
		if (imgs.length) {
			e.preventDefault();
			insertImages(imgs);
		}
	}

	function ondrop(e: DragEvent) {
		const imgs = imagesOf(e.dataTransfer?.files);
		if (imgs.length) {
			e.preventDefault();
			e.stopPropagation();
			insertImages(imgs);
		}
	}

	function onkeydown(e: KeyboardEvent) {
		const mod = e.metaKey || e.ctrlKey;
		if (mod && e.key.toLowerCase() === 'k') {
			e.preventDefault();
			link();
		}
	}

	const tools: { icon: IconName; title: string; run: () => void }[] = [
		{ icon: 'bold', title: 'Bold (⌘B)', run: () => cmd('bold') },
		{ icon: 'italic', title: 'Italic (⌘I)', run: () => cmd('italic') },
		{ icon: 'underline', title: 'Underline (⌘U)', run: () => cmd('underline') },
		{ icon: 'strike', title: 'Strikethrough', run: () => cmd('strikeThrough') },
		{ icon: 'list', title: 'Bulleted list', run: () => cmd('insertUnorderedList') },
		{ icon: 'listOrdered', title: 'Numbered list', run: () => cmd('insertOrderedList') },
		{ icon: 'quote', title: 'Quote', run: () => cmd('formatBlock', 'blockquote') },
		{ icon: 'link', title: 'Link (⌘K)', run: link },
		{ icon: 'eraser', title: 'Clear formatting', run: () => cmd('removeFormat') }
	];
</script>

<div class="rich">
	<div class="toolbar" role="toolbar" aria-label="Formatting">
		{#each tools as t (t.title)}
			<button type="button" title={t.title} aria-label={t.title} onmousedown={(e) => e.preventDefault()} onclick={t.run}
				><Icon name={t.icon} size={16} /></button
			>
		{/each}
		<label class="img" title="Insert image">
			<Icon name="image" size={16} />
			<input
				type="file"
				accept="image/png,image/jpeg,image/gif,image/webp"
				multiple
				class="sr-only"
				onchange={(e) => {
					insertImages(imagesOf(e.currentTarget.files));
					e.currentTarget.value = '';
				}}
			/>
		</label>
		{#if uploading}<span class="busy">Uploading image…</span>{/if}
	</div>
	<div
		bind:this={el}
		class="area"
		contenteditable="true"
		role="textbox"
		aria-multiline="true"
		aria-label="Message"
		tabindex="0"
		oninput={sync}
		{onpaste}
		{ondrop}
		{onkeydown}
	></div>
</div>

<style>
	.rich {
		flex: 1;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}
	.toolbar {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 2px;
		padding: 6px 12px;
		border-bottom: 1px solid var(--border);
	}
	.toolbar button,
	.img {
		display: inline-grid;
		place-items: center;
		min-width: 30px;
		height: 30px;
		padding: 0 6px;
		border: 0;
		border-radius: 6px;
		background: none;
		color: var(--text-soft);
		font-size: 14px;
		cursor: pointer;
	}
	.toolbar button:hover,
	.img:hover {
		background: var(--bg-hover);
		color: var(--text);
	}
	.busy {
		margin-left: 8px;
		font-size: 12px;
		color: var(--text-faint);
	}
	.area {
		flex: 1;
		min-height: 240px;
		padding: 16px 18px;
		overflow-y: auto;
		outline: none;
		line-height: 1.55;
		overflow-wrap: anywhere;
	}
	.area :global(blockquote) {
		margin: 0 0 0 0.25em;
		padding-left: 0.75em;
		border-left: 3px solid var(--border);
		color: var(--text-soft);
	}
	.area :global(img) {
		max-width: 100%;
		height: auto;
	}
</style>
