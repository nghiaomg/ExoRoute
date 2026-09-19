#!/usr/bin/env python3
"""Inventory a Rust file into top-level chunks (header + items)."""
import re
import sys

ITEM_RE = re.compile(
    r'^(pub(\([^)]*\))?\s+)?(async\s+|unsafe\s+|const\s+|static\s+|fn\s+|struct\s+|enum\s+|impl(\s*<[^>]*>)?\s+|trait\s+|type\s+|mod\s+)'
)
ATTR_RE = re.compile(r'^#\[.*\]')
USEMOD_RE = re.compile(r'^(pub(\([^)]*\))?\s+)?(use\s+|mod\s+|extern\s+)')

def name_of(first_line, kind):
    m = re.search(r'\b(fn|struct|enum|trait|type|mod|const|static)\s+([A-Za-z_0-9]+)', first_line)
    if m:
        return m.group(2)
    m = re.search(r'^impl(\s*<[^>]*>)?\s+(.+)', first_line)
    if m:
        return 'impl ' + m.group(2).strip()[:60]
    return first_line.strip()[:60]

def inventory(path):
    with open(path, encoding='utf-8') as f:
        lines = f.readlines()
    chunks = []
    cur = {'lines': [], 'start': 1}
    pending_attrs = []

    def flush():
        nonlocal cur
        if cur['lines']:
            chunks.append(cur)
        cur = {'lines': [], 'start': 0}

    i = 0
    n = len(lines)
    header_done = False
    while i < n:
        line = lines[i]
        s = line.strip()
        if not header_done and (USEMOD_RE.match(line) or ATTR_RE.match(line) or s == '' or s.startswith('//') or s.startswith('//!') or s.startswith('/*!') or s.startswith('*')):
            cur['lines'].append(line)
            if cur['start'] == 0:
                cur['start'] = i + 1
            i += 1
            continue
        if ITEM_RE.match(line) or ATTR_RE.match(line):
            if cur['lines']:
                flush()
                cur = {'lines': [], 'start': i + 1}
            elif cur['start'] == 0:
                cur['start'] = i + 1
            cur['lines'].append(line)
            header_done = True
            i += 1
            continue
        if s == '' or s.startswith('//'):
            # blank/comment lines: attach to current chunk if any, else keep for next
            if cur['lines']:
                cur['lines'].append(line)
            else:
                pending_attrs.append(line)
            i += 1
            continue
        # any other column-0 non-indented line that is not an item (e.g. closing brace)
        if cur['lines']:
            cur['lines'].append(line)
        else:
            if pending_attrs:
                cur['lines'].extend(pending_attrs)
                pending_attrs = []
            if cur['start'] == 0:
                cur['start'] = i + 1
            cur['lines'].append(line)
        header_done = True
        i += 1
    flush()
    # merge pending leading blanks
    total = 0
    print(f'=== {path} ({n} lines, {len(chunks)} chunks) ===')
    for idx, c in enumerate(chunks):
        end = c['start'] + len(c['lines']) - 1
        first = ''
        for ln in c['lines']:
            if ln.strip() and not ln.strip().startswith('//') and not ln.strip().startswith('#['):
                first = ln.strip()[:100]
                break
        kind = 'HEADER' if idx == 0 else name_of(first, '')
        print(f'[{idx}] lines {c["start"]}-{end} ({len(c["lines"])}): {kind} :: {first[:80]}')
        total += len(c['lines'])
    print(f'total accounted: {total}/{n}')

if __name__ == '__main__':
    for p in sys.argv[1:]:
        inventory(p)
