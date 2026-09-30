import fs from 'node:fs';
import path from 'node:path';
import { createMarkdownProcessor } from '@astrojs/markdown-remark';

const OWNER = 'naczo5';
const REPO = 'aoko-client';
const REPO_URL = `https://github.com/${OWNER}/${REPO}`;
const RELEASES_URL = `https://api.github.com/repos/${OWNER}/${REPO}/releases?per_page=100`;
const CACHE_FILE = path.resolve('.releases-cache.json');

const LATEST_FALLBACK = 'latest (main)';

export interface Release {
	tag: string;
	version: string;
	title: string;
	url: string;
	publishedAt: string | null;
	prerelease: boolean;
	latest: boolean;
	notesHtml: string;
}

interface GitHubRelease {
	tag_name: string;
	name: string | null;
	body: string | null;
	html_url: string;
	draft: boolean;
	prerelease: boolean;
	created_at: string;
	published_at: string | null;
}

function readCache(): GitHubRelease[] | null {
	try {
		const parsed = JSON.parse(fs.readFileSync(CACHE_FILE, 'utf-8'));
		return Array.isArray(parsed) ? parsed : null;
	} catch {
		return null;
	}
}

function writeCache(releases: GitHubRelease[]): void {
	try {
		fs.writeFileSync(CACHE_FILE, JSON.stringify(releases, null, '\t'));
	} catch {
		// A read-only checkout just means the next build refetches.
	}
}

async function fetchFromApi(): Promise<GitHubRelease[] | null> {
	const headers: Record<string, string> = {
		Accept: 'application/vnd.github+json',
		'User-Agent': 'aoko-docs-build',
		'X-GitHub-Api-Version': '2022-11-28',
	};
	const token = process.env.GITHUB_TOKEN;
	if (token) headers.Authorization = `Bearer ${token}`;

	const response = await fetch(RELEASES_URL, { headers });
	if (!response.ok) {
		console.warn(`[releases] GitHub API responded ${response.status} ${response.statusText}.`);
		return null;
	}

	const payload: unknown = await response.json();
	if (!Array.isArray(payload)) {
		console.warn('[releases] GitHub API returned an unexpected payload.');
		return null;
	}

	return payload as GitHubRelease[];
}

const COMPARE_LINK_LINE = /^\s*\*\*Full Changelog\*\*:.*$/i;

// GitHub's generated notes append a "Full Changelog" compare link, sometimes with
// prose after it. The release card links back to GitHub, so drop the line wherever
// it sits and keep the surrounding text. Bodies arrive with CRLF line endings.
function stripCompareLink(body: string): string {
	return body
		.replace(/\r\n?/g, '\n')
		.split('\n')
		.filter((line) => !COMPARE_LINK_LINE.test(line))
		.join('\n')
		.trim();
}

function toTimestamp(release: GitHubRelease): number {
	return Date.parse(release.published_at ?? release.created_at) || 0;
}

let pending: Promise<Release[]> | undefined;

// Both the changelog page and the landing page read releases, so fetch once per build.
export function getReleases(): Promise<Release[]> {
	pending ??= loadReleases();
	return pending;
}

async function loadReleases(): Promise<Release[]> {
	let payload = await fetchFromApi().catch((error: unknown) => {
		console.warn(`[releases] GitHub API request failed: ${(error as Error).message}`);
		return null;
	});

	if (payload) {
		writeCache(payload);
	} else {
		payload = readCache();
		if (payload) console.warn('[releases] Falling back to the cached release list.');
	}

	if (!payload) {
		console.warn('[releases] No release data available; the changelog will be empty.');
		return [];
	}

	const published = payload
		.filter((release) => !release.draft)
		.sort((a, b) => toTimestamp(b) - toTimestamp(a));

	const markdown = await createMarkdownProcessor({ syntaxHighlight: false });
	const latestTag = published.find((release) => !release.prerelease)?.tag_name;

	const releases: Release[] = [];
	for (const release of published) {
		const notes = stripCompareLink(release.body ?? '');
		releases.push({
			tag: release.tag_name,
			version: release.tag_name.replace(/^v/, ''),
			title: release.name?.trim() || release.tag_name,
			url: release.html_url,
			publishedAt: release.published_at,
			prerelease: release.prerelease,
			latest: release.tag_name === latestTag,
			notesHtml: notes ? (await markdown.render(notes)).code : '',
		});
	}

	return releases;
}

export function latestVersionLabel(releases: Release[]): string {
	const latest = releases.find((release) => release.latest);
	if (!latest) return LATEST_FALLBACK;
	const safe = latest.version.replace(/[^\w.+-]/g, '');
	return safe ? `v${safe}` : LATEST_FALLBACK;
}

export function formatReleaseDate(iso: string | null): string {
	if (!iso) return 'unreleased';
	const date = new Date(iso);
	if (Number.isNaN(date.getTime())) return 'unreleased';
	return new Intl.DateTimeFormat('en-GB', {
		day: 'numeric',
		month: 'long',
		year: 'numeric',
		timeZone: 'UTC',
	}).format(date);
}

export { REPO_URL };
