import ts from 'typescript';

/** Retain source positions and decoded keys before JSON.parse can discard duplicates. */
export function duplicateJsonKeys(text) {
  const source = ts.parseJsonText('resource.json', text);
  const duplicates = [];
  const lineAt = node => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;

  function visit(node, path) {
    if (!node) return;
    if (ts.isObjectLiteralExpression(node)) {
      const keys = new Map();
      for (const property of node.properties) {
        if (!ts.isPropertyAssignment(property) || !ts.isStringLiteral(property.name)) continue;
        const key = property.name.text;
        const childPath = path ? `${path}.${key}` : key;
        const line = lineAt(property.name);
        if (keys.has(key)) duplicates.push({ path: childPath, line, firstLine: keys.get(key) });
        else keys.set(key, line);
        visit(property.initializer, childPath);
      }
    } else if (ts.isArrayLiteralExpression(node)) {
      node.elements.forEach((child, index) => visit(child, `${path}[${index}]`));
    }
  }

  visit(source.statements[0]?.expression, '');
  return duplicates;
}
