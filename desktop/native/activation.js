(() => {
  try {
    if (process.env.DSH_DESKTOP_NATIVE_DISABLED === '1' || process.platform !== 'win32') return false;
    const fs = process.getBuiltinModule('fs'), p = process.getBuiltinModule('path');
    const root = p.dirname(p.dirname(p.resolve(process.argv[1])));
    const cli = JSON.parse(fs.readFileSync(p.join(root, 'package.json'), 'utf8'));
    if (cli.name !== '@deepseek-ai/dsh' || cli.version !== '0.1.2-rc.1') return false;
    return fs.existsSync(__NATIVE_ADDON__) && ['dsh-session', 'dsh-session-persistence', 'dsh-session-persistence-jsonl']
      .every(name => JSON.parse(fs.readFileSync(p.join(root, 'node_modules/@deepseek-ai', name, 'package.json'), 'utf8')).version === '0.1.2-rc.1');
  } catch { return false; }
})()
