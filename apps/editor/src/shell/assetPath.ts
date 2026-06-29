export function normalizeLogicalAssetPath(input: string): string {
  if (input.length === 0) {
    throw new Error('empty');
  }
  if (input.startsWith('//') || input.startsWith('\\\\')) {
    throw new Error('UNC');
  }
  if (input.startsWith('/') || input.startsWith('\\')) {
    throw new Error('root absolute');
  }

  if (/^[A-Za-z]:/.test(input)) {
    const afterDrive = input[2];
    if (afterDrive === '/' || afterDrive === '\\') {
      throw new Error('absolute path');
    }
    throw new Error('drive-relative');
  }

  const segments: string[] = [];
  for (const segment of input.split(/[\\/]/u)) {
    if (segment.length === 0 || segment === '.') {
      continue;
    }
    if (segment === '..') {
      if (segments.length === 0) {
        throw new Error('escapes root');
      }
      throw new Error('contains ..');
    }
    segments.push(segment);
  }

  if (segments[0] === 'Assets') {
    segments.shift();
  }

  if (segments.length === 0) {
    throw new Error('empty');
  }

  return segments.join('/');
}
