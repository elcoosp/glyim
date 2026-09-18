header = '// Assembled modular glyim standard library (Option A).\n'
out = header
core_modules = ['option', 'result', 'iter', 'slice', 'str', 'cell', 'mem', 'ptr', 'ops', 'cmp', 'marker', 'panic', 'hint', 'convert', 'default', 'future']
for name in core_modules:
    src = open('crates/glyim-lang-core/lib/'+name+'.g').read()
    out += '// === module: '+name+' ===\n' + src + '\n'

# Check several offsets
for off, hi in [(601,604), (791,797), (899,905), (1225,1231), (1442,1445), (2349,2368), (3280,3287)]:
    text = out[off:hi]
    lines = out.splitlines(True)
    pos = 0
    lineno = 0
    col = 0
    for i, line in enumerate(lines, 1):
        if pos <= off < pos + len(line):
            lineno = i
            col = off - pos
            break
        pos += len(line)
    print(f"off={off} hi={hi} line={lineno} col={col}: {repr(text)}")
