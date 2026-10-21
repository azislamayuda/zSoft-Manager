#!/usr/bin/env bash
# Optional LIVE audit. Does not download or update any installed application.
set -euo pipefail
python3 - <<'PY'
import json
import urllib.error
import urllib.request

repos = (
    'PDFCraftTool/pdfcraft', 'TriliumNext/Trilium',
    'obsidianmd/obsidian-releases', 'OpenSelena/omniget',
    'github/app', 'shiftkey/desktop', 'Autonomy-Logic/openplc-editor'
)
headers = {'User-Agent': 'zSoft-Manager/0.4.4', 'Accept': 'application/vnd.github+json'}

def api(repo, path):
    request = urllib.request.Request(f'https://api.github.com/repos/{repo}/{path}', headers=headers)
    with urllib.request.urlopen(request, timeout=30) as stream:
        return json.load(stream)

for repo in repos:
    print(f'\n=== {repo} ===')
    try:
        release = api(repo, 'releases/latest')
        items = [a['name'] for a in release.get('assets', [])
                 if a.get('name', '').lower().endswith('.appimage')]
        tag = release.get('tag_name', '(unknown tag)')
        if items:
            print('Latest release:', tag)
            for asset in items: print('  ', asset)
        else:
            print(f'Latest release {tag} has no AppImage; checking up to 12 recent releases')
            for older in api(repo, 'releases?per_page=12'):
                if older.get('draft') or older.get('prerelease'): continue
                names = [a['name'] for a in older.get('assets', [])
                         if a.get('name', '').lower().endswith('.appimage')]
                if names:
                    print('Candidate release:', older.get('tag_name'))
                    for asset in names: print('  ', asset)
                    break
            else:
                print('No AppImage found in inspected releases')
    except (urllib.error.URLError, ValueError, KeyError) as exc:
        print('GitHub API error:', exc)
PY
