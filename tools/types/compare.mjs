/**
 * Compare yuzuko.d.ts with a reference declaration file.
 *
 * Shared types, values, and functions are compared after interface heritage is
 * flattened. A finding means the reference has a member yuzuko.d.ts lacks, or its
 * spelling is more specific: AllowSharedBufferSource, a TypeScript 7 typed-array
 * argument, or a tighter generic constraint. Yuzuko stays quiet when it is
 * already more specific (a string literal for `string`, an extra union member,
 * Float32Array<ArrayBufferLike> for a bare Float32Array). Number aliases,
 * ReadonlyArray, Array<T>, union order, and Iterable-versus-array spellings
 * match. An extra `undefined` on a reference union is wider, so it is quiet.
 * A type alias of a local interface keeps that alias name and still matches a
 * use of the target interface. An interface that extends the reference type is
 * more specific. A reference name the reference file does not declare matches
 * this file's definition of that name. `GPUAllowSharedBufferSource` matches
 * `AllowSharedBufferSource`. A DOM union member whose type this file does not
 * declare is ignored. Iterator aliases with the same type arguments match.
 * AlgorithmIdentifier covers a more specific algorithm union. Types that exist
 * on only one side are not findings. Window is skipped because the Yuzuko
 * declaration is an empty stub. The `@webgpu/types` pass skips nominal
 * `__brand` members and `new (): never` constructors.
 *
 * Parsing uses the TypeScript 7 IPC client. `typescript/sync` keeps one
 * compiler process for the run; each comparison builds a program over an
 * in-memory file and reads syntactic diagnostics from it. The AST guards come
 * from `typescript/ast`.
 */
import { API, DiagnosticCategory } from "typescript/sync";
import { createFileSystem } from "typescript/fs";
import * as ts from "typescript/ast";

let api;

function compiler() {
  if (!api) {
    api = new API();
    process.once("exit", () => {
      try {
        api?.[Symbol.dispose]();
      } catch {
        // The compiler process has already exited.
      }
    });
  }
  return api;
}

const EMPTY_ALLOW = {
  skipTypes: new Set(),
  skipMembers: new Set(),
  skipMismatches: new Set(),
  skipOverloads: [],
  skipBrand: false,
  skipNeverNew: false,
};

// compareTexts is synchronous, so this context stays stable for one comparison.
let active = { yuzukoNames: new Set(), web: null, yuzuko: null, label: "@types/web" };

/**
 * @param {string} text
 * @param {{ skipTypes?: string[], groups?: { members?: string[], mismatches?: string[] }[], members?: string[], mismatches?: string[], skipOverloads?: { member: string, params: number, contains?: string }[], webgpu?: { groups?: { members?: string[], mismatches?: string[] }[], members?: string[], mismatches?: string[], skipOverloads?: { member: string, params: number, contains?: string }[] } }} json
 * @param {{ webgpu?: boolean }} [options]
 */
export function allowFromJson(json, options = {}) {
  const skipMembers = new Set(json.members ?? []);
  const skipMismatches = new Set(json.mismatches ?? []);
  const skipOverloads = [...(json.skipOverloads ?? [])];
  const groups = [...(json.groups ?? [])];
  if (options.webgpu) groups.push(...(json.webgpu?.groups ?? []));
  for (const group of groups) {
    for (const name of group.members ?? []) skipMembers.add(name);
    for (const name of group.mismatches ?? []) skipMismatches.add(name);
  }
  if (options.webgpu) {
    for (const name of json.webgpu?.members ?? []) skipMembers.add(name);
    for (const name of json.webgpu?.mismatches ?? []) skipMismatches.add(name);
    skipOverloads.push(...(json.webgpu?.skipOverloads ?? []));
  }
  return {
    skipTypes: new Set(json.skipTypes ?? []),
    skipMembers,
    skipMismatches,
    skipOverloads,
    skipBrand: Boolean(options.webgpu),
    skipNeverNew: Boolean(options.webgpu),
  };
}

/**
 * @param {string} yuzukoText
 * @param {string} webText
 * @param {ReturnType<typeof allowFromJson>} [allow]
 * @param {string} [label] Reference name used in finding notes.
 */
export function compareTexts(yuzukoText, webText, allow = EMPTY_ALLOW, label = "@types/web") {
  const yuzuko = parse(yuzukoText, "yuzuko.d.ts");
  let web;
  try {
    web = parse(webText, "web.d.ts");
    return diff(yuzuko, web, allow, label);
  } finally {
    yuzuko.release();
    web?.release();
  }
}

function parse(text, fileName) {
  const virtualName = `/virtual/${fileName}`;
  const snapshot = compiler().createSnapshot({
    fileSystem: createFileSystem([[virtualName, text]]),
    createPrograms: [{
      rootFiles: [virtualName],
      compilerOptions: { noLib: true },
    }],
  });
  try {
    const program = snapshot.operation.createdPrograms?.[0];
    const source = program?.getSourceFile(virtualName);
    if (!source) throw new Error(`${fileName}: TypeScript did not return a source file`);
    const first = program.getSyntacticDiagnostics(virtualName).find((diagnostic) => diagnostic.category === DiagnosticCategory.Error);
    if (first) {
      const where = source.getLineAndCharacterOfPosition(first.pos ?? 0);
      throw new Error(`${fileName}:${where.line + 1}: ${first.text}`);
    }
    const types = new Map();
    const values = new Map();
    const functions = new Map();
    for (const statement of source.statements) {
      collect(statement, source, types, values, functions);
    }
    const surface = { source, types, values, functions };
    finalize(surface);
    let released = false;
    surface.release = () => {
      if (released) return;
      released = true;
      snapshot[Symbol.dispose]();
    };
    return surface;
  } catch (error) {
    snapshot[Symbol.dispose]();
    throw error;
  }
}

function collect(statement, source, types, values, functions) {
  if (ts.isInterfaceDeclaration(statement) && statement.name) {
    const decl = objectDecl(statement.name.text, types);
    decl.heritage.push(...heritage(statement));
    if (decl.typeParams.length === 0) decl.typeParams.push(...typeParamNodes(statement));
    decl.ownMembers.push(...membersOf(statement.members, source, false));
  } else if (ts.isClassDeclaration(statement) && statement.name) {
    const name = statement.name.text;
    const decl = objectDecl(name, types);
    decl.heritage.push(...heritage(statement));
    if (decl.typeParams.length === 0) decl.typeParams.push(...typeParamNodes(statement));
    const instance = [];
    const statics = [];
    for (const member of statement.members) {
      if (ts.isSemicolonClassElement(member)) continue;
      if (ts.isConstructorDeclaration(member)) {
        statics.push(signatureMember("new", "constructor", [member], source, name, decl.typeParams));
        continue;
      }
      const parsed = membersOf([member], source, true);
      if (hasModifier(member, ts.SyntaxKind.StaticKeyword)) statics.push(...parsed);
      else instance.push(...parsed);
    }
    if (!statics.some((member) => member.name === "new")) {
      statics.push(signatureMember("new", "constructor", [], source, name, decl.typeParams));
    }
    decl.ownMembers.push(...instance);
    const value = valueDecl(name, values);
    value.members.push(...statics);
  } else if (ts.isTypeAliasDeclaration(statement) && statement.name) {
    types.set(statement.name.text, {
      kind: "alias",
      name: statement.name.text,
      typeParams: typeParamNodes(statement),
      body: statement.type,
    });
  } else if (ts.isFunctionDeclaration(statement) && statement.name) {
    const name = statement.name.text;
    const list = functions.get(name) ?? [];
    list.push(readSignature(statement, source));
    functions.set(name, list);
  } else if (ts.isVariableStatement(statement)) {
    for (const declarator of statement.declarationList.declarations) {
      if (!ts.isIdentifier(declarator.name) || !declarator.type) continue;
      const value = valueDecl(declarator.name.text, values);
      value.members.push(...membersFromType(declarator.type, source));
      value.type = declarator.type;
    }
  } else if (ts.isModuleDeclaration(statement) && ts.isIdentifier(statement.name) && statement.body && ts.isModuleBlock(statement.body)) {
    const value = valueDecl(statement.name.text, values);
    for (const inner of statement.body.statements) {
      if (!ts.isVariableStatement(inner)) continue;
      for (const declarator of inner.declarationList.declarations) {
        if (!ts.isIdentifier(declarator.name)) continue;
        const readonly = hasModifier(inner, ts.SyntaxKind.ConstKeyword) || (inner.declarationList.flags & ts.NodeFlags.Const) !== 0;
        value.members.push({
          name: declarator.name.text,
          kind: "property",
          optional: false,
          readonly,
          typeNode: declarator.type ?? null,
          signatures: [],
          text: textOf(declarator, source),
        });
      }
    }
    value.type = null;
  } else if (ts.isEnumDeclaration(statement)) {
    const value = valueDecl(statement.name.text, values);
    for (const member of statement.members) {
      if (!ts.isIdentifier(member.name) && !ts.isStringLiteral(member.name)) continue;
      const name = ts.isIdentifier(member.name) ? member.name.text : member.name.text;
      value.members.push({
        name,
        kind: "property",
        optional: false,
        readonly: true,
        typeNode: null,
        literal: member.initializer ?? null,
        signatures: [],
        text: textOf(member, source),
      });
    }
    value.type = null;
  }
}

function objectDecl(name, types) {
  const existing = types.get(name);
  if (existing?.kind === "object") return existing;
  const decl = { kind: "object", name, heritage: [], typeParams: [], ownMembers: [] };
  types.set(name, decl);
  return decl;
}

function valueDecl(name, values) {
  const existing = values.get(name);
  if (existing) return existing;
  const decl = { name, members: [], type: null };
  values.set(name, decl);
  return decl;
}

function heritage(node) {
  const found = [];
  for (const clause of node.heritageClauses ?? []) {
    if (clause.token !== ts.SyntaxKind.ExtendsKeyword) continue;
    for (const type of clause.types) {
      found.push({
        name: entityName(type.typeName ?? type.expression),
        args: type.typeArguments ? [...type.typeArguments] : [],
      });
    }
  }
  return found;
}

function entityName(node) {
  if (ts.isIdentifier(node)) return node.text;
  if (ts.isQualifiedName(node)) return `${entityName(node.left)}.${entityName(node.right)}`;
  if (ts.isPropertyAccessExpression(node)) return textOf(node, node.getSourceFile());
  return textOf(node, node.getSourceFile());
}

function typeParamNodes(node) {
  return (node.typeParameters ?? []).map((param) => ({
    name: param.name.text,
    constraint: param.constraint ?? null,
    defaultType: param.defaultType ?? param.default ?? null,
  }));
}

function membersOf(nodes, source, fromClass) {
  const out = [];
  for (const node of nodes) {
    if (ts.isPropertySignatureDeclaration(node) || (fromClass && ts.isPropertyDeclaration(node))) {
      if (!node.name) continue;
      out.push({
        name: propertyName(node.name, source),
        kind: "property",
        optional: Boolean(node.questionToken),
        readonly: hasModifier(node, ts.SyntaxKind.ReadonlyKeyword),
        typeNode: node.type ?? null,
        signatures: [],
        text: textOf(node, source),
      });
    } else if (ts.isMethodSignatureDeclaration(node) || (fromClass && ts.isMethodDeclaration(node))) {
      if (!node.name) continue;
      out.push({
        name: propertyName(node.name, source),
        kind: "method",
        optional: Boolean(node.questionToken),
        readonly: false,
        typeNode: null,
        signatures: [readSignature(node, source)],
        text: textOf(node, source),
      });
    } else if (ts.isConstructSignatureDeclaration(node)) {
      out.push(signatureMember("new", "constructor", [node], source));
    } else if (ts.isCallSignatureDeclaration(node)) {
      out.push(signatureMember("call", "call", [node], source));
    } else if (ts.isIndexSignatureDeclaration(node)) {
      const key = node.parameters[0]?.type;
      const keyName = key ? textOf(key, source) : "index";
      out.push({
        name: `[${keyName}]`,
        kind: "index",
        optional: false,
        readonly: hasModifier(node, ts.SyntaxKind.ReadonlyKeyword),
        typeNode: node.type ?? null,
        signatures: [],
        text: textOf(node, source),
      });
    } else if (ts.isGetAccessorDeclaration(node) && node.name) {
      out.push({
        name: propertyName(node.name, source),
        kind: "get",
        optional: false,
        readonly: true,
        typeNode: node.type ?? null,
        signatures: [],
        text: textOf(node, source),
      });
    } else if (ts.isSetAccessorDeclaration(node) && node.name) {
      out.push({
        name: propertyName(node.name, source),
        kind: "set",
        optional: false,
        readonly: false,
        typeNode: node.parameters[0]?.type ?? null,
        signatures: [],
        text: textOf(node, source),
      });
    }
  }
  return out;
}

function signatureMember(name, kind, nodes, source, className, classTypeParams) {
  const signatures = nodes.map((node) => readSignature(node, source, kind === "constructor" ? className : undefined, classTypeParams));
  if (signatures.length === 0 && className) {
    signatures.push({
      typeParams: [],
      thisType: null,
      params: [],
      returnNode: null,
      synthesizedReturn: {
        k: "ref",
        v: className,
        a: (classTypeParams ?? []).map((_, index) => ({ k: "tparam", i: index })),
      },
      text: `new(): ${className}`,
    });
  }
  return {
    name,
    kind,
    optional: false,
    readonly: false,
    typeNode: null,
    signatures,
    text: signatures.map((signature) => signature.text).join(" / "),
  };
}

function membersFromType(typeNode, source) {
  if (!ts.isTypeLiteralNode(typeNode)) return [];
  return membersOf(typeNode.members, source, false);
}

function readSignature(node, source, className, classTypeParams) {
  const params = [];
  let thisType = null;
  for (const param of node.parameters ?? []) {
    if (ts.isIdentifier(param.name) && param.name.text === "this") {
      thisType = param.type ?? null;
      continue;
    }
    params.push({
      name: paramName(param.name, source),
      optional: Boolean(param.questionToken || param.initializer),
      rest: Boolean(param.dotDotDotToken),
      typeNode: param.type ?? null,
    });
  }
  let returnNode = node.type ?? null;
  let synthesizedReturn = null;
  if (!returnNode && className) {
    synthesizedReturn = {
      k: "ref",
      v: className,
      a: (classTypeParams ?? []).map((_, index) => ({ k: "tparam", i: index })),
    };
  }
  return {
    typeParams: typeParamNodes(node),
    thisType,
    params,
    returnNode,
    synthesizedReturn,
    text: textOf(node, source),
  };
}

function propertyName(node, source) {
  if (ts.isIdentifier(node) || ts.isStringLiteral(node) || ts.isNumericLiteral(node)) return node.text;
  if (ts.isComputedPropertyName(node)) return textOf(node.expression, source).replace(/\s+/g, "");
  return textOf(node, source);
}

function paramName(node, source) {
  if (ts.isIdentifier(node)) return node.text;
  return textOf(node, source);
}

function hasModifier(node, kind) {
  return (node.modifiers ?? []).some((modifier) => modifier.kind === kind);
}

function textOf(node, source) {
  return node.getText(source).replace(/\s+/g, " ").trim();
}

function finalize(surface) {
  for (const decl of surface.types.values()) {
    if (decl.kind !== "object") continue;
    decl.ownMembers = combineAccessors(mergeMembers(decl.ownMembers));
  }
  for (const value of surface.values.values()) {
    value.members = combineAccessors(mergeMembers(value.members)).filter((member) => member.name !== "prototype");
    if (value.type && ts.isTypeLiteralNode(value.type)) value.type = null;
  }
  const aliasNames = new Map();
  for (const [name, decl] of surface.types) {
    if (decl.kind === "alias") aliasNames.set(name, decl);
  }
  surface.aliases = aliasNames;
  for (const decl of surface.types.values()) {
    if (decl.kind === "object") {
      const env = envFor(surface, decl.typeParams, new Set());
      decl.members = decl.ownMembers.map((member) => canonMember(member, env));
      decl.params = decl.typeParams.map((param, index) => canonParam(param, envFor(surface, decl.typeParams.slice(0, index), new Set())));
    } else {
      const env = envFor(surface, decl.typeParams, new Set([decl.name]));
      decl.canon = canonType(decl.body, env);
      decl.params = decl.typeParams.map((param, index) => canonParam(param, envFor(surface, decl.typeParams.slice(0, index), new Set([decl.name]))));
      decl.text = textOf(decl.body, surface.source);
    }
  }
  for (const value of surface.values.values()) {
    const env = envFor(surface, [], new Set());
    value.canonMembers = value.members.map((member) => canonMember(member, env));
    value.canonType = value.type ? canonType(value.type, env) : null;
  }
  for (const [name, signatures] of surface.functions) {
    const env = envFor(surface, [], new Set());
    surface.functions.set(name, signatures.map((signature) => canonSignature(signature, env)));
  }
  for (const decl of surface.types.values()) {
    if (decl.kind !== "object") continue;
    decl.flat = flatten(surface, decl.name, new Set());
  }
}

function envFor(surface, typeParams, stack) {
  const params = new Map();
  typeParams.forEach((param, index) => params.set(param.name, { k: "tparam", i: index }));
  return { surface, params, stack };
}

function canonParam(param, env) {
  return {
    constraint: param.constraint ? canonType(param.constraint, env) : null,
    defaultType: param.defaultType ? canonType(param.defaultType, env) : null,
  };
}

function mergeMembers(members) {
  const merged = [];
  for (const member of members) {
    const index = merged.findIndex((item) => item.name === member.name && item.kind === member.kind);
    if (index < 0) {
      merged.push({ ...member, signatures: [...(member.signatures ?? [])] });
      continue;
    }
    const current = merged[index];
    if (member.kind === "method" || member.kind === "constructor" || member.kind === "call") {
      current.signatures.push(...member.signatures);
      current.text = `${current.text} / ${member.text}`;
      current.optional = current.optional && member.optional;
    }
  }
  return merged;
}

function combineAccessors(members) {
  const accessors = new Map();
  const out = [];
  for (const member of members) {
    if (member.kind !== "get" && member.kind !== "set") {
      out.push(member);
      continue;
    }
    const slot = accessors.get(member.name) ?? { get: null, set: null };
    slot[member.kind === "get" ? "get" : "set"] = member;
    accessors.set(member.name, slot);
  }
  for (const [name, slot] of accessors) {
    out.push({
      name,
      kind: "property",
      optional: false,
      readonly: !slot.set,
      typeNode: slot.get?.typeNode ?? slot.set?.typeNode ?? null,
      signatures: [],
      text: slot.get?.text ?? slot.set?.text ?? name,
    });
  }
  return out;
}

function canonMember(member, env) {
  return {
    name: member.name,
    kind: member.kind,
    optional: member.optional,
    readonly: member.readonly,
    text: member.text,
    type: member.typeNode ? canonType(member.typeNode, env) : member.literal ? canonType(literalish(member.literal), env) : null,
    signatures: (member.signatures ?? []).map((signature) => canonSignature(signature, env)),
  };
}

function literalish(node) {
  return node;
}

function canonSignature(signature, env) {
  const params = new Map(env.params);
  const local = [];
  signature.typeParams.forEach((param, index) => {
    const placeholder = { k: "mparam", i: index };
    params.set(param.name, placeholder);
    local.push(param);
  });
  const localEnv = { ...env, params };
  return {
    text: signature.text,
    typeParams: local.map((param, index) => canonParam(param, { ...localEnv, params: scopedParams(env.params, local, index) })),
    thisType: signature.thisType ? canonType(signature.thisType, localEnv) : null,
    params: signature.params.map((param) => ({
      name: param.name,
      optional: param.optional,
      rest: param.rest,
      type: param.typeNode ? canonType(param.typeNode, localEnv) : { k: "kw", v: "any" },
    })),
    returnType: signature.synthesizedReturn ?? (signature.returnNode ? canonType(signature.returnNode, localEnv) : { k: "kw", v: "void" }),
  };
}

function scopedParams(outer, locals, count) {
  const params = new Map(outer);
  for (let index = 0; index < count; index += 1) {
    params.set(locals[index].name, { k: "mparam", i: index });
  }
  return params;
}

function canonType(node, env) {
  if (!node) return { k: "kw", v: "any" };
  if (node.k === "ref" || node.k === "lit" || node.k === "kw") return node;
  const keyword = KEYWORDS[node.kind];
  if (keyword) return { k: "kw", v: keyword };
  if (ts.isLiteralTypeNode(node)) return canonLiteral(node.literal);
  if (ts.isParenthesizedTypeNode(node)) return canonType(node.type, env);
  if (ts.isUnionTypeNode(node)) return unionOf(node.types.map((part) => canonType(part, env)));
  if (ts.isIntersectionTypeNode(node)) return intersectionOf(node.types.map((part) => canonType(part, env)));
  if (ts.isArrayTypeNode(node)) return { k: "arr", ro: false, e: canonType(node.elementType, env) };
  if (ts.isTupleTypeNode(node)) {
    return {
      k: "tuple",
      ro: false,
      e: node.elements.map((element) => {
        if (ts.isNamedTupleMember(element)) {
          return { e: canonType(element.type, env), opt: Boolean(element.questionToken), rest: Boolean(element.dotDotDotToken) };
        }
        if (ts.isOptionalTypeNode(element)) return { e: canonType(element.type, env), opt: true, rest: false };
        if (ts.isRestTypeNode(element)) return { e: canonType(element.type, env), opt: false, rest: true };
        return { e: canonType(element, env), opt: false, rest: false };
      }),
    };
  }
  if (ts.isTypeOperatorNode(node)) {
    const inner = canonType(node.type, env);
    if (node.operator === ts.SyntaxKind.ReadonlyKeyword) {
      if (inner.k === "arr" || inner.k === "tuple") return { ...inner, ro: true };
      return { k: "readonly", t: inner };
    }
    if (node.operator === ts.SyntaxKind.KeyOfKeyword) return { k: "keyof", t: inner };
    if (node.operator === ts.SyntaxKind.UniqueKeyword) return { k: "unique", t: inner };
  }
  if (ts.isTypeReferenceNode(node)) return canonReference(node, env);
  if (ts.isFunctionTypeNode(node) || ts.isConstructorTypeNode(node)) {
    return { k: "fn", ctor: ts.isConstructorTypeNode(node), ...canonSignature(readSignature(node, node.getSourceFile()), env) };
  }
  if (ts.isTypeLiteralNode(node)) {
    const members = combineAccessors(mergeMembers(membersOf(node.members, node.getSourceFile(), false))).map((member) => canonMember(member, env));
    return { k: "obj", m: members.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) };
  }
  if (ts.isTypeQueryNode(node)) return { k: "typeof", v: textOf(node.exprName, node.getSourceFile()).replace(/\s+/g, "") };
  if (ts.isIndexedAccessTypeNode(node)) {
    return { k: "indexed", o: canonType(node.objectType, env), i: canonType(node.indexType, env) };
  }
  if (ts.isConditionalTypeNode(node)) {
    return {
      k: "cond",
      c: canonType(node.checkType, env),
      x: canonType(node.extendsType, env),
      y: canonType(node.trueType, env),
      n: canonType(node.falseType, env),
    };
  }
  if (ts.isInferTypeNode(node)) return { k: "infer", v: node.typeParameter.name.text };
  if (ts.isTemplateLiteralTypeNode(node)) {
    return {
      k: "tpl",
      h: node.head.text,
      s: node.templateSpans.map((span) => ({ t: canonType(span.type, env), l: span.literal.text })),
    };
  }
  if (ts.isThisTypeNode(node)) return { k: "this" };
  if (ts.isTypePredicateNode(node)) {
    return { k: "pred", asserts: Boolean(node.assertsModifier), t: node.type ? canonType(node.type, env) : null };
  }
  if (ts.isImportTypeNode(node) || ts.isMappedTypeNode(node)) {
    return { k: "raw", v: normalizeRaw(textOf(node, node.getSourceFile())) };
  }
  return { k: "raw", v: normalizeRaw(textOf(node, node.getSourceFile())) };
}

const KEYWORDS = {
  [ts.SyntaxKind.AnyKeyword]: "any",
  [ts.SyntaxKind.UnknownKeyword]: "unknown",
  [ts.SyntaxKind.NumberKeyword]: "number",
  [ts.SyntaxKind.BigIntKeyword]: "bigint",
  [ts.SyntaxKind.ObjectKeyword]: "object",
  [ts.SyntaxKind.BooleanKeyword]: "boolean",
  [ts.SyntaxKind.StringKeyword]: "string",
  [ts.SyntaxKind.SymbolKeyword]: "symbol",
  [ts.SyntaxKind.VoidKeyword]: "void",
  [ts.SyntaxKind.UndefinedKeyword]: "void",
  [ts.SyntaxKind.NeverKeyword]: "never",
};

function canonLiteral(literal) {
  if (ts.isStringLiteral(literal)) return { k: "lit", t: "string", v: literal.text };
  if (ts.isNumericLiteral(literal)) return { k: "lit", t: "number", v: numberValue(literal.text) };
  if (literal.kind === ts.SyntaxKind.TrueKeyword) return { k: "lit", t: "boolean", v: true };
  if (literal.kind === ts.SyntaxKind.FalseKeyword) return { k: "lit", t: "boolean", v: false };
  if (literal.kind === ts.SyntaxKind.NullKeyword) return { k: "lit", t: "null", v: null };
  if (ts.isPrefixUnaryExpression(literal) && literal.operator === ts.SyntaxKind.MinusToken && ts.isNumericLiteral(literal.operand)) {
    const value = numberValue(literal.operand.text);
    return { k: "lit", t: "number", v: typeof value === "number" ? -value : `-${value}` };
  }
  if (ts.isBigIntLiteral(literal)) return { k: "lit", t: "bigint", v: literal.text };
  return { k: "raw", v: normalizeRaw(literal.getText()) };
}

function numberValue(text) {
  const value = Number(text);
  if (!Number.isFinite(value) || Math.abs(value) > Number.MAX_SAFE_INTEGER) return text;
  return value;
}

function canonReference(node, env) {
  const name = entityName(node.typeName);
  const args = (node.typeArguments ?? []).map((arg) => canonType(arg, env));
  if (env.params.has(name) && args.length === 0) return env.params.get(name);
  if (name === "ReadonlyArray" && args.length === 1) return { k: "arr", ro: true, e: args[0] };
  if (name === "Array" && args.length === 1) return { k: "arr", ro: false, e: args[0] };
  const callback = callbackFunction(env, name, args);
  if (callback) return callback;
  const alias = env.surface.aliases.get(name);
  if (!alias || env.stack.has(name)) return { k: "ref", v: name, a: args };
  const interfaceTarget = localInterfaceAlias(env.surface, alias, new Set(env.stack));
  if (interfaceTarget && args.length === 0) return { k: "ref", v: name, a: [], aliasOf: interfaceTarget };
  const params = new Map(env.params);
  alias.typeParams.forEach((param, index) => {
    if (args[index]) params.set(param.name, args[index]);
    else if (param.defaultType) params.set(param.name, canonType(param.defaultType, { ...env, params, stack: new Set(env.stack).add(name) }));
    else params.set(param.name, { k: "tparam", i: index });
  });
  const stack = new Set(env.stack);
  stack.add(name);
  return canonType(alias.body, { surface: env.surface, params, stack });
}

function callbackFunction(env, name, args) {
  if (env.stack.has(name)) return null;
  const decl = env.surface.types.get(name);
  if (!decl || decl.kind !== "object" || decl.ownMembers.length === 0) return null;
  if (!decl.ownMembers.every((member) => member.kind === "call" || member.kind === "constructor")) return null;
  const params = new Map(env.params);
  decl.typeParams.forEach((param, index) => {
    params.set(param.name, args[index] ?? { k: "tparam", i: index });
  });
  const local = { surface: env.surface, params, stack: new Set(env.stack).add(name) };
  const signatures = decl.ownMembers.flatMap((member) => member.signatures.map((signature) => canonSignature(signature, local)));
  if (signatures.length !== 1) return null;
  return { k: "fn", ctor: decl.ownMembers[0].kind === "constructor", ...signatures[0] };
}

function unionOf(parts) {
  const flat = [];
  for (const part of parts) {
    if (part.k === "union") flat.push(...part.a);
    else flat.push(part);
  }
  const seen = new Set();
  const out = [];
  for (const part of flat) {
    const key = sortKey(part);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(part);
  }
  out.sort((a, b) => (sortKey(a) < sortKey(b) ? -1 : 1));
  if (out.length === 1) return out[0];
  return { k: "union", a: out };
}

function intersectionOf(parts) {
  const flat = [];
  for (const part of parts) {
    if (part.k === "inter") flat.push(...part.a);
    else flat.push(part);
  }
  const out = [];
  const seen = new Set();
  for (const part of flat) {
    const key = sortKey(part);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(part);
  }
  out.sort((a, b) => (sortKey(a) < sortKey(b) ? -1 : 1));
  if (out.length === 1) return out[0];
  return { k: "inter", a: out };
}

function sortKey(type) {
  return JSON.stringify(type, (key, value) => (key === "aliasOf" ? undefined : value));
}

function localInterfaceAlias(surface, alias, stack) {
  if (!alias || stack.has(alias.name) || alias.typeParams.length > 0) return null;
  const body = alias.body;
  if (!ts.isTypeReferenceNode(body) || (body.typeArguments?.length ?? 0) > 0) return null;
  stack.add(alias.name);
  const target = entityName(body.typeName);
  const decl = surface.types.get(target);
  if (decl?.kind === "object") return target;
  return localInterfaceAlias(surface, surface.aliases.get(target), stack);
}

function flatten(surface, name, stack) {
  if (stack.has(name)) return [];
  const decl = surface.types.get(name);
  if (!decl || decl.kind !== "object") return [];
  stack.add(name);
  const map = new Map();
  for (const base of decl.heritage) {
    const baseDecl = surface.types.get(base.name);
    const inherited = flatten(surface, base.name, stack);
    const args = base.args.map((arg) => canonType(arg, envFor(surface, decl.typeParams, new Set())));
    for (const member of inherited) {
      map.set(member.name, substituteMember(member, args));
    }
    if (!baseDecl) {
      // A base this file does not declare, such as Error, contributes nothing.
    }
  }
  for (const member of decl.members) map.set(member.name, member);
  stack.delete(name);
  return [...map.values()];
}

function substituteMember(member, args) {
  if (args.length === 0) return member;
  return {
    ...member,
    type: member.type ? substitute(member.type, args) : null,
    signatures: member.signatures.map((signature) => ({
      ...signature,
      typeParams: signature.typeParams.map((param) => ({
        constraint: param.constraint ? substitute(param.constraint, args) : null,
        defaultType: param.defaultType ? substitute(param.defaultType, args) : null,
      })),
      thisType: signature.thisType ? substitute(signature.thisType, args) : null,
      params: signature.params.map((param) => ({ ...param, type: substitute(param.type, args) })),
      returnType: substitute(signature.returnType, args),
    })),
  };
}

function substitute(type, args) {
  if (!type || typeof type !== "object") return type;
  if (type.k === "tparam") return args[type.i] ?? type;
  if (Array.isArray(type)) return type.map((item) => substitute(item, args));
  const next = {};
  for (const [key, value] of Object.entries(type)) {
    next[key] = key === "k" || key === "v" || key === "t" || typeof value !== "object" ? value : substitute(value, args);
  }
  return next;
}

function diff(yuzuko, web, allow, label) {
  const previous = active;
  active = { yuzukoNames: new Set(yuzuko.types.keys()), web, yuzuko, label };
  try {
    return diffSurfaces(yuzuko, web, allow);
  } finally {
    active = previous;
  }
}

function diffSurfaces(yuzuko, web, allow) {
  const findings = [];
  const sharedTypes = [...web.types.keys()].filter((name) => yuzuko.types.has(name)).sort();
  for (const name of sharedTypes) {
    if (allow.skipTypes.has(name)) continue;
    const webDecl = web.types.get(name);
    const yuzukoDecl = yuzuko.types.get(name);
    if (webDecl.kind === "alias" && yuzukoDecl.kind === "alias") {
      compareAlias(name, webDecl, yuzukoDecl, findings);
      continue;
    }
    if (webDecl.kind === "object" && yuzukoDecl.kind === "object") {
      compareParams(name, webDecl.params, yuzukoDecl.params, findings);
      compareMemberLists(name, webDecl.flat, yuzukoDecl.flat, web, yuzuko, webDecl, yuzukoDecl, allow, findings);
      continue;
    }
    const webShape = shapeOf(webDecl);
    const yuzukoShape = shapeOf(yuzukoDecl);
    if (webShape && yuzukoShape && (relateObject(webShape, yuzukoShape, 0) === "equal" || relateObject(webShape, yuzukoShape, 0) === "yuzuko")) {
      continue;
    }
    if (webDecl.kind === "alias" && yuzukoDecl.kind === "object" && interfaceExtendsType(yuzuko, yuzukoDecl, webDecl.canon)) {
      continue;
    }
    if (allow.skipMismatches.has(name)) continue;
    findings.push({
      path: name,
      kind: "type",
      web: webDecl.kind === "alias" ? webDecl.text : `interface ${name}`,
      yuzuko: yuzukoDecl.kind === "alias" ? yuzukoDecl.text : `interface ${name}`,
      note: "One side declares a type alias and the other declares an interface.",
    });
  }
  const sharedValues = [...web.values.keys()].filter((name) => yuzuko.values.has(name)).sort();
  for (const name of sharedValues) {
    if (allow.skipTypes.has(name)) continue;
    const webValue = web.values.get(name);
    const yuzukoValue = yuzuko.values.get(name);
    if (webValue.canonType && yuzukoValue.canonType && webValue.canonMembers.length === 0 && yuzukoValue.canonMembers.length === 0) {
      const relation = relate(webValue.canonType, yuzukoValue.canonType);
      if (relation === "dom" || relation === "mix") {
        if (allow.skipMismatches.has(name)) continue;
        findings.push(typeFinding(name, webValue.canonType, yuzukoValue.canonType, textOf(webValue.type, web.source), textOf(yuzukoValue.type, yuzuko.source)));
      }
      continue;
    }
    compareMemberLists(name, webValue.canonMembers, yuzukoValue.canonMembers, web, yuzuko, null, null, allow, findings);
  }
  const sharedFunctions = [...web.functions.keys()].filter((name) => yuzuko.functions.has(name)).sort();
  for (const name of sharedFunctions) {
    compareOverloads(name, web.functions.get(name), yuzuko.functions.get(name), allow, findings);
  }
  findings.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : a.kind < b.kind ? -1 : 1));
  return {
    findings,
    stats: { types: sharedTypes.length, values: sharedValues.length, functions: sharedFunctions.length },
  };
}

function compareAlias(name, webDecl, yuzukoDecl, findings) {
  compareParams(name, webDecl.params, yuzukoDecl.params, findings);
  const relation = relate(webDecl.canon, yuzukoDecl.canon);
  if (relation === "dom" || relation === "mix") {
    findings.push(typeFinding(name, webDecl.canon, yuzukoDecl.canon, webDecl.text, yuzukoDecl.text));
  }
}

function compareParams(path, webParams, yuzukoParams, findings) {
  const count = Math.max(webParams.length, yuzukoParams.length);
  for (let index = 0; index < count; index += 1) {
    const webParam = webParams[index];
    const yuzukoParam = yuzukoParams[index];
    if (!webParam || !yuzukoParam) {
      findings.push({
        path: `${path} type parameter ${index + 1}`,
        kind: "type",
        web: webParam ? describeParam(webParam) : "(none)",
        yuzuko: yuzukoParam ? describeParam(yuzukoParam) : "(none)",
        note: "The type parameter lists differ.",
      });
      continue;
    }
    const relation = combine(
      relateOptional(webParam.constraint, yuzukoParam.constraint),
      relateOptional(webParam.defaultType, yuzukoParam.defaultType),
    );
    if (relation === "dom" || relation === "mix") {
      findings.push({
        path: `${path} type parameter ${index + 1}`,
        kind: "type",
        web: describeParam(webParam),
        yuzuko: describeParam(yuzukoParam),
        note: noteFor(webParam.constraint, yuzukoParam.constraint),
      });
    }
  }
}

function describeParam(param) {
  const constraint = param.constraint ? ` extends ${display(param.constraint)}` : "";
  const fallback = param.defaultType ? ` = ${display(param.defaultType)}` : "";
  return `T${constraint}${fallback}`;
}

function relateOptional(web, yuzuko) {
  if (!web && !yuzuko) return "equal";
  if (web && !yuzuko) return "dom";
  if (!web && yuzuko) return "yuzuko";
  return relate(web, yuzuko);
}

function compareMemberLists(owner, webMembers, yuzukoMembers, web, yuzuko, webDecl, yuzukoDecl, allow, findings) {
  const yuzukoByName = new Map(yuzukoMembers.map((member) => [member.name, member]));
  for (const webMember of webMembers) {
    if (allow.skipBrand && webMember.name === "__brand") continue;
    const path = `${owner}.${webMember.name}`;
    const yuzukoMember = yuzukoByName.get(webMember.name);
    if (!yuzukoMember) {
      if (allow.skipMembers.has(path)) continue;
      if (webDecl && yuzukoDecl && reportedOnBase(web, yuzuko, webDecl.name, yuzukoDecl.name, webMember.name)) continue;
      findings.push({
        path,
        kind: "missing",
        web: webMember.text,
        yuzuko: null,
        note: null,
      });
      continue;
    }
    const webSignatures = signaturesOf(webMember);
    const yuzukoSignatures = signaturesOf(yuzukoMember);
    if (webSignatures.length > 0 || yuzukoSignatures.length > 0) {
      if (webSignatures.length === 0 || yuzukoSignatures.length === 0) {
        findings.push(typeFinding(path, webMember.type, yuzukoMember.type, webMember.text, yuzukoMember.text));
        continue;
      }
      const optional = optionalRelation(webMember.optional, yuzukoMember.optional);
      if (optional === "dom" && !allow.skipMismatches.has(path)) {
        findings.push({
          path,
          kind: "type",
          web: webMember.text,
          yuzuko: yuzukoMember.text,
          note: `The ${active.label} member is required.`,
        });
      }
      compareOverloads(path, webSignatures, yuzukoSignatures, allow, findings);
      continue;
    }
    const relation = combine(
      relate(webMember.type ?? { k: "kw", v: "any" }, yuzukoMember.type ?? { k: "kw", v: "any" }),
      readonlyRelation(webMember.readonly, yuzukoMember.readonly),
      optionalRelation(webMember.optional, yuzukoMember.optional),
    );
    if ((relation === "dom" || relation === "mix") && !allow.skipMismatches.has(path)) {
      findings.push(typeFinding(path, webMember.type, yuzukoMember.type, webMember.text, yuzukoMember.text));
    }
  }
}

function signaturesOf(member) {
  if (member.signatures.length > 0) return member.signatures;
  if (member.type?.k === "fn") return [member.type];
  return [];
}

function reportedOnBase(web, yuzuko, webName, yuzukoName, memberName) {
  const origin = declaredOn(web, webName, memberName, new Set());
  if (!origin || origin === webName) return false;
  return heritageIncludes(yuzuko, yuzukoName, origin, new Set());
}

function declaredOn(surface, name, memberName, stack) {
  if (stack.has(name)) return null;
  const decl = surface.types.get(name);
  if (!decl || decl.kind !== "object") return null;
  if (decl.members.some((member) => member.name === memberName)) return name;
  stack.add(name);
  for (const base of decl.heritage) {
    const found = declaredOn(surface, base.name, memberName, stack);
    if (found) return found;
  }
  return null;
}

function heritageIncludes(surface, name, target, stack) {
  if (name === target) return true;
  if (stack.has(name)) return false;
  const decl = surface.types.get(name);
  if (!decl || decl.kind !== "object") return false;
  stack.add(name);
  return decl.heritage.some((base) => heritageIncludes(surface, base.name, target, stack));
}

function shapeOf(decl) {
  if (decl.kind === "object") return { k: "obj", m: decl.flat ?? decl.members };
  if (decl.kind === "alias" && decl.canon?.k === "obj") return decl.canon;
  if (decl.kind === "alias" && decl.canon?.k === "ref") {
    const target = active.yuzuko?.types.get(decl.canon.v) ?? active.web?.types.get(decl.canon.v);
    if (target?.kind === "object") return { k: "obj", m: target.flat ?? target.members };
  }
  return null;
}

function compareOverloads(path, webSignatures, yuzukoSignatures, allow, findings) {
  const yuzukoNorm = yuzukoSignatures.map((signature) => normalizeListener(path, signature));
  for (const signature of webSignatures) {
    if (isEventMapOverload(path, signature)) continue;
    if (allow.skipNeverNew && path.endsWith(".new") && signature.returnType?.k === "kw" && signature.returnType.v === "never") continue;
    if (covers(normalizeListener(path, signature), yuzukoNorm)) continue;
    if (overloadSkipped(allow, path, signature)) continue;
    if (allow.skipMismatches.has(path)) continue;
    findings.push({
      path,
      kind: webSignatures.length > 1 || yuzukoSignatures.length > 0 ? "overload" : "type",
      web: signature.text,
      yuzuko: yuzukoSignatures.map((item) => item.text).join(" / ") || null,
      note: noteForSignature(signature, yuzukoSignatures[0]),
    });
  }
}

function covers(web, signatures, depth = 0) {
  if (signatures.some((candidate) => coveredBy(web, candidate, false))) return true;
  if (depth > 2) return false;
  const index = web.params.findIndex((param) => param.type?.k === "union" && param.type.a.length > 1 && param.type.a.every(isLiteral));
  if (index < 0) return false;
  return web.params[index].type.a.every((part) => signatures.some((candidate) => {
    const split = {
      ...web,
      params: web.params.map((param, paramIndex) => (paramIndex === index ? { ...param, type: part } : param)),
    };
    return coveredBy(split, candidate, true);
  }));
}

function coveredBy(web, candidate, allowNarrowerReturn) {
  const expanded = expandParamExcludes(candidate);
  const relation = relateSignature(web, expanded);
  if (relation === "equal" || relation === "yuzuko") return true;
  if (!allowNarrowerReturn) return false;
  const params = relateSignature({ ...web, returnType: expanded.returnType }, expanded);
  if (params !== "equal" && params !== "yuzuko") return false;
  const back = relate(expanded.returnType, web.returnType);
  return back === "equal" || back === "yuzuko";
}

function expandParamExcludes(signature) {
  return {
    ...signature,
    params: signature.params.map((param) => ({ ...param, type: expandExclude(param.type) })),
  };
}

function expandExclude(type) {
  if (type?.k !== "ref" || type.v !== "Exclude" || type.a.length < 2) return type;
  const source = asParts(type.a[0]);
  const removedParts = asParts(type.a[1]);
  if (!source.every(isLiteral) || !removedParts.every(isLiteral)) return type;
  const removed = new Set(removedParts.map(sortKey));
  const kept = source.filter((part) => !removed.has(sortKey(part)));
  if (kept.length === 0 || kept.length === source.length) return type;
  return unionOf(kept);
}

function overloadSkipped(allow, path, signature) {
  return allow.skipOverloads.some((entry) => {
    if (entry.member !== path || entry.params !== signature.params.length) return false;
    return !entry.contains || signature.text.includes(entry.contains);
  });
}

function normalizeListener(path, signature) {
  const name = path.split(".").pop();
  if (name !== "addEventListener" && name !== "removeEventListener") return signature;
  return {
    ...signature,
    params: signature.params.map((param, index) => (index === 1 ? { ...param, type: { k: "kw", v: "any" } } : param)),
  };
}

function isEventMapOverload(path, signature) {
  const name = path.split(".").pop();
  if (name !== "addEventListener" && name !== "removeEventListener") return false;
  return signature.typeParams.some((param) => param.constraint?.k === "keyof");
}

function relateSignature(web, yuzuko) {
  let relation = "equal";
  const count = Math.max(web.typeParams.length, yuzuko.typeParams.length);
  for (let index = 0; index < count; index += 1) {
    relation = combine(relation, relateOptional(web.typeParams[index]?.constraint, yuzuko.typeParams[index]?.constraint));
    relation = combine(relation, relateOptional(web.typeParams[index]?.defaultType, yuzuko.typeParams[index]?.defaultType));
    if (!web.typeParams[index] || !yuzuko.typeParams[index]) relation = combine(relation, web.typeParams[index] ? "dom" : "yuzuko");
  }
  if (web.thisType && yuzuko.thisType && !isUnknown(web.thisType, active.yuzukoNames)) {
    relation = combine(relation, relate(web.thisType, yuzuko.thisType));
  }
  const params = Math.max(web.params.length, yuzuko.params.length);
  for (let index = 0; index < params; index += 1) {
    const webParam = web.params[index];
    const yuzukoParam = yuzuko.params[index];
    if (!yuzukoParam) {
      if (webParam.optional && webParam.name === "thisArg" && webParam.type.k === "kw" && webParam.type.v === "any") continue;
      return "dom";
    }
    if (!webParam) {
      relation = combine(relation, yuzukoParam.optional || yuzukoParam.rest ? "yuzuko" : "yuzuko");
      continue;
    }
    if (webParam.rest !== yuzukoParam.rest) return "mix";
    let webType = expandExclude(webParam.type);
    let yuzukoType = expandExclude(yuzukoParam.type);
    if (webParam.optional !== yuzukoParam.optional) {
      if (!webParam.optional && yuzukoParam.optional && hasVoid(webType)) webType = stripVoid(webType);
      else if (webParam.optional && !yuzukoParam.optional && hasVoid(yuzukoType)) yuzukoType = stripVoid(yuzukoType);
      else relation = combine(relation, yuzukoParam.optional ? "dom" : "yuzuko");
    }
    relation = combine(relation, relate(webType, yuzukoType));
  }
  relation = combine(relation, relate(web.returnType, yuzuko.returnType));
  return relation;
}

function noteForSignature(web, yuzuko) {
  if (!yuzuko) return null;
  for (let index = 0; index < web.params.length; index += 1) {
    const note = noteFor(web.params[index]?.type, yuzuko.params[index]?.type);
    if (note) return note;
  }
  for (let index = 0; index < web.typeParams.length; index += 1) {
    const note = noteFor(web.typeParams[index]?.constraint, yuzuko.typeParams[index]?.constraint);
    if (note) return note;
  }
  return noteFor(web.returnType, yuzuko.returnType);
}

function readonlyRelation(web, yuzuko) {
  if (web === yuzuko) return "equal";
  return web ? "dom" : "yuzuko";
}

function optionalRelation(webOptional, yuzukoOptional) {
  if (webOptional === yuzukoOptional) return "equal";
  return yuzukoOptional ? "dom" : "yuzuko";
}

function typeFinding(path, web, yuzuko, webText, yuzukoText) {
  const literals = missingLiterals(web, yuzuko);
  const note = literals.length > 0 ? `Missing ${literals.map((item) => display(item)).join(", ")}.` : noteFor(web, yuzuko);
  return { path, kind: "type", web: webText, yuzuko: yuzukoText, note };
}

function missingLiterals(web, yuzuko) {
  const webParts = unionParts(web);
  const yuzukoParts = unionParts(yuzuko);
  if (!webParts || !yuzukoParts) return [];
  if (!webParts.every(isLiteral) || !yuzukoParts.every(isLiteral)) return [];
  const have = new Set(yuzukoParts.map(sortKey));
  return webParts.filter((part) => !have.has(sortKey(part)));
}

function unionParts(type) {
  if (!type) return null;
  if (type.k === "union") return type.a;
  if (type.k === "lit") return [type];
  return null;
}

function isLiteral(type) {
  return type.k === "lit";
}

function noteFor(web, yuzuko) {
  if (!web || !yuzuko) return null;
  const webBuffer = bufferKind(web);
  const yuzukoBuffer = bufferKind(yuzuko);
  if (webBuffer === "allow" && yuzukoBuffer === "buffer") {
    return "AllowSharedBufferSource accepts a SharedArrayBuffer. BufferSource does not.";
  }
  if (web.k === "ref" && yuzuko.k === "ref" && web.v === yuzuko.v && web.a.length > 0 && yuzuko.a.length === 0) {
    return `The ${active.label} type argument is the TypeScript 7 spelling.`;
  }
  const webArg = bufferArgKind(web);
  const yuzukoArg = bufferArgKind(yuzuko);
  if (webArg === "ab" && yuzukoArg === "like") {
    return `The ${active.label} type argument is ArrayBuffer, which excludes a SharedArrayBuffer.`;
  }
  if (constraintText(web).includes("Exclude<BufferSource, ArrayBuffer>") && !constraintText(yuzuko).includes("Exclude<BufferSource, ArrayBuffer>")) {
    return `The ${active.label} constraint uses Exclude<BufferSource, ArrayBuffer>.`;
  }
  return null;
}

function constraintText(type) {
  return type ? display(type) : "";
}

function display(type) {
  if (!type) return "";
  switch (type.k) {
    case "kw":
      return type.v;
    case "lit":
      if (type.t === "string") return JSON.stringify(type.v);
      if (type.t === "boolean" || type.t === "number") return String(type.v);
      if (type.t === "null") return "null";
      return String(type.v);
    case "ref":
      return type.a.length > 0 ? `${type.v}<${type.a.map(display).join(", ")}>` : type.v;
    case "arr":
      return `${type.ro ? "readonly " : ""}${display(type.e)}[]`;
    case "union":
      return type.a.map(display).join(" | ");
    case "inter":
      return type.a.map(display).join(" & ");
    case "tparam":
    case "mparam":
      return `T${type.i}`;
    default:
      return type.v ?? type.k;
  }
}

function combine(...relations) {
  let result = "equal";
  for (const relation of relations) {
    if (!relation || relation === "equal") continue;
    if (result === "equal") result = relation;
    else if (result !== relation) return "mix";
  }
  return result;
}

function relate(web, yuzuko, depth = 0) {
  if (!web || !yuzuko) return web || yuzuko ? "mix" : "equal";
  if (depth > 40) return sortKey(web) === sortKey(yuzuko) ? "equal" : "mix";
  web = normIterable(web);
  yuzuko = normIterable(yuzuko);
  if (sortKey(web) === sortKey(yuzuko)) return "equal";
  const buffers = bufferRelation(web, yuzuko);
  if (buffers) return buffers;
  const args = bufferArgRelation(web, yuzuko);
  if (args) return args;
  const arrays = arrayRelation(web, yuzuko, depth);
  if (arrays) return arrays;
  if (web.k === "kw" && web.v === "any" && !(yuzuko.k === "kw" && yuzuko.v === "any")) return "yuzuko";
  if (yuzuko.k === "kw" && yuzuko.v === "any" && !(web.k === "kw" && web.v === "any")) return "dom";
  if (web.k === "kw" && isLiteralFamily(yuzuko, web.v)) return "yuzuko";
  if (yuzuko.k === "kw" && isLiteralFamily(web, yuzuko.v)) return "dom";
  if (isAlgorithm(web) && isAlgorithmDetail(yuzuko)) return "yuzuko";
  if (isAlgorithm(yuzuko) && isAlgorithmDetail(web)) return "yuzuko";
  const numeric = numericSourceRelation(web, yuzuko);
  if (numeric) return numeric;
  const coefficients = coefficientRelation(web, yuzuko);
  if (coefficients) return coefficients;
  const iterators = iteratorRelation(web, yuzuko, depth);
  if (iterators) return iterators;
  const refs = refRelation(web, yuzuko);
  if (refs) return refs;
  if (web.k === "ref") {
    const unresolved = unresolvedRelation(web, yuzuko);
    if (unresolved) return unresolved;
  }
  if ((web.k === "ref" && yuzuko.k === "obj") || (web.k === "obj" && yuzuko.k === "ref")) {
    const webShape = web.k === "obj" ? web : interfaceShape(web, active.web);
    const yuzukoShape = yuzuko.k === "obj" ? yuzuko : interfaceShape(yuzuko, active.yuzuko);
    if (webShape && yuzukoShape) return relateObject(webShape, yuzukoShape, depth + 1);
  }
  if (web.k === "union" || yuzuko.k === "union") return relateUnion(asParts(web), asParts(yuzuko), depth);
  if (web.k === "inter" || yuzuko.k === "inter") return relateIntersection(asInter(web), asInter(yuzuko), depth);
  if (web.k === "ref" && yuzuko.k === "ref" && web.v === yuzuko.v) {
    if (web.a.length === yuzuko.a.length) {
      return combine(...web.a.map((arg, index) => relate(arg, yuzuko.a[index], depth + 1)));
    }
    if (web.a.length > 0 && yuzuko.a.length === 0) return "dom";
    if (yuzuko.a.length > 0 && web.a.length === 0) return "yuzuko";
    return "mix";
  }
  if (web.k === "fn" && yuzuko.k === "fn") return relateSignature(web, yuzuko);
  if (web.k === "obj" && yuzuko.k === "obj") return relateObject(web, yuzuko, depth);
  if (web.k === "tuple" && yuzuko.k === "tuple") return relateTuple(web, yuzuko, depth);
  if (web.k === "readonly" && yuzuko.k === "readonly") return relate(web.t, yuzuko.t, depth + 1);
  if (web.k === "keyof" && yuzuko.k === "keyof") return relate(web.t, yuzuko.t, depth + 1);
  if (web.k === "indexed" && yuzuko.k === "indexed") {
    return combine(relate(web.o, yuzuko.o, depth + 1), relate(web.i, yuzuko.i, depth + 1));
  }
  return "mix";
}

function normIterable(type) {
  if (type.k === "ref" && type.v === "Iterable" && type.a.length === 1) {
    return { k: "arr", ro: false, iter: true, e: type.a[0] };
  }
  return type;
}

function arrayRelation(web, yuzuko, depth) {
  if (web.k !== "arr" || yuzuko.k !== "arr") return null;
  const elements = relate(web.e, yuzuko.e, depth + 1);
  if (web.iter || yuzuko.iter) return elements;
  if (web.ro === yuzuko.ro) return elements;
  return combine(elements, web.ro ? "dom" : "yuzuko");
}

function bufferRelation(web, yuzuko) {
  const webKind = bufferKind(web);
  const yuzukoKind = bufferKind(yuzuko);
  if (!webKind || !yuzukoKind) return null;
  if (webKind === yuzukoKind) return "equal";
  if (webKind === "allow" && yuzukoKind === "buffer") return "dom";
  if (webKind === "buffer" && yuzukoKind === "allow") return "yuzuko";
  return null;
}

function bufferKind(type) {
  if (type?.k === "ref" && (type.v === "AllowSharedBufferSource" || type.v === "GPUAllowSharedBufferSource")) return "allow";
  if (type?.k === "ref" && type.v === "BufferSource") return "buffer";
  const parts = asParts(type);
  const shared = parts.filter((part) => part.k === "ref" && part.v === "SharedArrayBuffer");
  if (shared.length > 0) {
    const rest = parts.filter((part) => !(part.k === "ref" && part.v === "SharedArrayBuffer"));
    const restType = rest.length === 1 ? rest[0] : { k: "union", a: rest };
    const restKind = bufferKind(restType);
    if (restKind === "buffer" || restKind === "allow") return "allow";
  }
  const kinds = parts.map(partKind);
  if (kinds.some((kind) => kind === "other")) return null;
  const set = new Set(kinds);
  if (set.has("like") && set.has("view-like") && set.size === 2) return "allow";
  if (set.has("ab") && set.has("view-ab") && set.size === 2) return "buffer";
  return null;
}

function partKind(type) {
  if (type.k === "ref" && type.v === "ArrayBuffer" && type.a.length === 0) return "ab";
  if (type.k === "ref" && type.v === "SharedArrayBuffer" && type.a.length === 0) return "sab";
  if (type.k === "ref" && type.v === "ArrayBufferLike" && type.a.length === 0) return "like";
  if (type.k === "ref" && (type.v === "ArrayBufferView" || TYPED_ARRAYS.has(type.v))) {
    if (type.a.length === 0) return "view-bare";
    const arg = bufferArgKind(type.a[0]);
    if (arg === "ab") return "view-ab";
    if (arg === "like") return "view-like";
    return "other";
  }
  if (bufferArgKind(type) === "like") return "like";
  return "other";
}

const TYPED_ARRAYS = new Set([
  "Int8Array",
  "Uint8Array",
  "Uint8ClampedArray",
  "Int16Array",
  "Uint16Array",
  "Int32Array",
  "Uint32Array",
  "Float32Array",
  "Float64Array",
  "BigInt64Array",
  "BigUint64Array",
  "DataView",
]);

function bufferArgKind(type) {
  if (!type) return null;
  if (type.k === "ref" && type.a.length === 0 && type.v === "ArrayBuffer") return "ab";
  if (type.k === "ref" && type.a.length === 0 && type.v === "ArrayBufferLike") return "like";
  if (type.k === "ref" && type.a.length === 0 && type.v === "SharedArrayBuffer") return "sab";
  if (type.k === "union") {
    const names = type.a.map((part) => (part.k === "ref" ? part.v : ""));
    if (new Set(names).size === 2 && names.includes("ArrayBuffer") && names.includes("SharedArrayBuffer")) return "like";
  }
  return null;
}

function bufferArgRelation(web, yuzuko) {
  const webKind = bufferArgKind(web);
  const yuzukoKind = bufferArgKind(yuzuko);
  if (!webKind || !yuzukoKind || webKind === "sab" || yuzukoKind === "sab") return null;
  if (webKind === yuzukoKind) return "equal";
  if (webKind === "ab" && yuzukoKind === "like") return "dom";
  if (webKind === "like" && yuzukoKind === "ab") return "yuzuko";
  return null;
}

function asParts(type) {
  if (type.k === "union") return type.a;
  return [type];
}

function asInter(type) {
  if (type.k === "inter") return type.a;
  return [type];
}

const LIB_NAMES = new Set([
  "Array",
  "ArrayBuffer",
  "ArrayBufferLike",
  "ArrayBufferView",
  "ArrayIterator",
  "AsyncIterable",
  "AsyncIterator",
  "Awaited",
  "BigInt",
  "Boolean",
  "DataView",
  "Date",
  "Error",
  "Exclude",
  "Extract",
  "FinalizationRegistry",
  "Float32Array",
  "Float64Array",
  "Function",
  "IArguments",
  "Int8Array",
  "Int16Array",
  "Int32Array",
  "Iterable",
  "IterableIterator",
  "Iterator",
  "Map",
  "NonNullable",
  "Number",
  "Object",
  "Omit",
  "Partial",
  "Pick",
  "Promise",
  "PromiseLike",
  "PropertyKey",
  "ReadonlyArray",
  "ReadonlyMap",
  "ReadonlySet",
  "Record",
  "RegExp",
  "Required",
  "ReturnType",
  "Set",
  "SharedArrayBuffer",
  "String",
  "Symbol",
  "TemplateStringsArray",
  "Uint8Array",
  "Uint8ClampedArray",
  "Uint16Array",
  "Uint32Array",
  "WeakMap",
  "WeakRef",
  "WeakSet",
  ...TYPED_ARRAYS,
]);

const ITERATOR_NAMES = new Set([
  "ArrayIterator",
  "AsyncIterableIterator",
  "AsyncIterator",
  "FormDataIterator",
  "HeadersIterator",
  "IterableIterator",
  "Iterator",
  "IteratorObject",
  "ReadableStreamAsyncIterator",
  "URLSearchParamsIterator",
]);

function isUnknown(type, names) {
  if (!type) return false;
  if (type.k === "ref") return !names.has(type.v) && !LIB_NAMES.has(type.v);
  if (type.k === "arr") return isUnknown(type.e, names);
  if (type.k === "union" || type.k === "inter") return type.a.every((part) => isUnknown(part, names));
  return false;
}

function interfaceShape(type, surface) {
  const decl = surface?.types.get(type.v);
  if (!decl || decl.kind !== "object") return null;
  const members = (decl.flat ?? decl.members).map((member) => substituteMember(member, type.a));
  return { k: "obj", m: members };
}

function coefficientRelation(web, yuzuko) {
  if (isSpecificTypedArray(yuzuko) && isLooserCoefficient(web, yuzuko.v)) return "yuzuko";
  if (isSpecificTypedArray(web) && isLooserCoefficient(yuzuko, web.v)) return "dom";
  return null;
}

function isSpecificTypedArray(type) {
  return type.k === "ref" && TYPED_ARRAYS.has(type.v) && type.a.length > 0;
}

function isLooserCoefficient(type, name) {
  return asParts(type).every((part) => {
    if (part.k === "arr" && part.e.k === "kw" && (part.e.v === "number" || part.e.v === "bigint")) return true;
    return part.k === "ref" && part.v === name && part.a.length === 0;
  });
}

function iteratorRelation(web, yuzuko, depth) {
  if (web.k !== "ref" || yuzuko.k !== "ref") return null;
  if (!ITERATOR_NAMES.has(web.v) || !ITERATOR_NAMES.has(yuzuko.v)) return null;
  if (web.a.length !== yuzuko.a.length) return null;
  return combine(...web.a.map((arg, index) => relate(arg, yuzuko.a[index], depth + 1)));
}

function refRelation(web, yuzuko) {
  if (web.k !== "ref" || yuzuko.k !== "ref") return null;
  if (web.aliasOf && web.aliasOf === yuzuko.v && yuzuko.a.length === 0) return "equal";
  if (yuzuko.aliasOf && yuzuko.aliasOf === web.v && web.a.length === 0) return "yuzuko";
  if (web.aliasOf && yuzuko.aliasOf && web.aliasOf === yuzuko.aliasOf && web.a.length === yuzuko.a.length) return "equal";
  if (interfaceExtendsType(active.yuzuko, active.yuzuko?.types.get(yuzuko.v), web)) return "yuzuko";
  if (interfaceExtendsType(active.web, active.web?.types.get(web.v), yuzuko)) return "dom";
  return null;
}

function interfaceExtendsType(surface, decl, type) {
  if (!surface || !decl || decl.kind !== "object" || type?.k !== "ref") return false;
  return decl.heritage.some((base) => sortKey(heritageCanon(surface, decl, base)) === sortKey(type));
}

function heritageCanon(surface, decl, base) {
  const args = base.args.map((arg) => canonType(arg, envFor(surface, decl.typeParams, new Set())));
  return { k: "ref", v: base.name, a: args };
}

function unresolvedRelation(web, yuzuko) {
  const surface = active.web;
  if (!surface || surface.types.has(web.v) || surface.aliases.has(web.v) || LIB_NAMES.has(web.v)) return null;
  const decl = active.yuzuko?.types.get(web.v);
  if (!decl) return null;
  if (yuzuko.v === web.v && sortKey(yuzuko.a) === sortKey(web.a)) return "equal";
  if (decl.kind === "alias" && sortKey(decl.canon) === sortKey(yuzuko)) return "yuzuko";
  return null;
}

function relateUnion(webParts, yuzukoParts, depth) {
  if (webParts.some(isVoidKeyword) && !yuzukoParts.some(isVoidKeyword)) {
    const rest = webParts.filter((part) => !isVoidKeyword(part));
    if (rest.length === 0) return "yuzuko";
    return combine(relateUnion(rest, yuzukoParts, depth), "yuzuko");
  }
  if (!(webParts.every(isLiteral) && yuzukoParts.every(isLiteral))) {
    webParts = webParts.filter((part) => !isUnknown(part, active.yuzukoNames));
    if (webParts.length === 0) return yuzukoParts.length === 0 ? "equal" : "yuzuko";
  }
  if (webParts.every(isLiteral) && yuzukoParts.every(isLiteral)) {
    const webKeys = new Set(webParts.map(sortKey));
    const yuzukoKeys = new Set(yuzukoParts.map(sortKey));
    const missing = [...webKeys].some((key) => !yuzukoKeys.has(key));
    const extra = [...yuzukoKeys].some((key) => !webKeys.has(key));
    if (!missing && !extra) return "equal";
    if (!missing) return "yuzuko";
    if (!extra) return "dom";
    return "mix";
  }
  const used = new Set();
  let relation = "equal";
  for (const part of webParts) {
    let match = -1;
    let matchRelation = "mix";
    for (let index = 0; index < yuzukoParts.length; index += 1) {
      if (used.has(index)) continue;
      const candidate = relate(part, yuzukoParts[index], depth + 1);
      if (candidate === "equal" || candidate === "yuzuko") {
        match = index;
        matchRelation = candidate;
        if (candidate === "equal") break;
      }
    }
    if (match < 0) return webParts.length === yuzukoParts.length ? "mix" : "dom";
    used.add(match);
    relation = combine(relation, matchRelation);
  }
  if (used.size < yuzukoParts.length) relation = combine(relation, "yuzuko");
  return relation;
}

function relateIntersection(webParts, yuzukoParts, depth) {
  const used = new Set();
  let relation = "equal";
  for (const part of webParts) {
    let match = -1;
    for (let index = 0; index < yuzukoParts.length; index += 1) {
      if (used.has(index)) continue;
      const candidate = relate(part, yuzukoParts[index], depth + 1);
      if (candidate === "equal" || candidate === "yuzuko") {
        match = index;
        relation = combine(relation, candidate);
        break;
      }
    }
    if (match < 0) return "dom";
    used.add(match);
  }
  if (used.size < yuzukoParts.length) relation = combine(relation, "yuzuko");
  return relation;
}

function relateObject(web, yuzuko, depth) {
  const yuzukoMembers = new Map(yuzuko.m.map((member) => [member.name, member]));
  let relation = "equal";
  for (const member of web.m) {
    const other = yuzukoMembers.get(member.name);
    if (!other) return "dom";
    if (member.signatures.length > 0 || other.signatures.length > 0) {
      relation = combine(relation, member.signatures.every((signature) => other.signatures.some((candidate) => {
        const result = relateSignature(signature, candidate);
        return result === "equal" || result === "yuzuko";
      })) ? "equal" : "dom");
      continue;
    }
    relation = combine(
      relation,
      relate(member.type ?? { k: "kw", v: "any" }, other.type ?? { k: "kw", v: "any" }, depth + 1),
      readonlyRelation(member.readonly, other.readonly),
      optionalRelation(member.optional, other.optional),
    );
  }
  if (yuzuko.m.length > web.m.length) relation = combine(relation, "yuzuko");
  return relation;
}

function relateTuple(web, yuzuko, depth) {
  if (web.e.length !== yuzuko.e.length) return "mix";
  let relation = web.ro === yuzuko.ro ? "equal" : web.ro ? "dom" : "yuzuko";
  for (let index = 0; index < web.e.length; index += 1) {
    relation = combine(relation, relate(web.e[index].e, yuzuko.e[index].e, depth + 1));
    if (web.e[index].opt !== yuzuko.e[index].opt) relation = combine(relation, yuzuko.e[index].opt ? "dom" : "yuzuko");
    if (web.e[index].rest !== yuzuko.e[index].rest) return "mix";
  }
  return relation;
}

function isLiteralFamily(type, family) {
  const parts = type.k === "union" ? type.a : [type];
  return parts.length > 0 && parts.every((part) => part.k === "lit" && (family === "boolean" ? part.t === "boolean" : part.t === family));
}

function isAlgorithm(type) {
  const parts = asParts(type);
  if (parts.length !== 2) return false;
  const names = parts.map((part) => (part.k === "kw" ? part.v : part.k === "ref" ? part.v : ""));
  return names.includes("string") && names.includes("Algorithm");
}

function isAlgorithmDetail(type) {
  const parts = asParts(type);
  if (parts.length === 0) return false;
  return parts.every((part) => {
    if (part.k === "kw" && part.v === "string") return true;
    if (part.k === "lit" && part.t === "string") return true;
    if (part.k === "obj") return true;
    if (part.k !== "ref") return false;
    return part.v === "Algorithm" || /(?:Params|Algorithm)$/.test(part.v);
  });
}

function isVoidKeyword(type) {
  return type?.k === "kw" && type.v === "void";
}

function hasVoid(type) {
  if (!type) return false;
  if (isVoidKeyword(type)) return true;
  return type.k === "union" && type.a.some(isVoidKeyword);
}

function stripVoid(type) {
  if (!type || type.k !== "union") return type;
  const parts = type.a.filter((part) => !isVoidKeyword(part));
  if (parts.length === type.a.length) return type;
  if (parts.length === 1) return parts[0];
  return { k: "union", a: parts };
}

const NUMERIC_TYPED_ARRAYS = new Set([
  "Float32Array",
  "Float64Array",
  "Int8Array",
  "Int16Array",
  "Int32Array",
  "Uint8Array",
  "Uint8ClampedArray",
  "Uint16Array",
  "Uint32Array",
]);

function numericSourceRelation(web, yuzuko) {
  if (isNumericIterable(yuzuko) && isBareNumericSource(web)) return "yuzuko";
  if (isNumericIterable(web) && isBareNumericSource(yuzuko)) return "yuzuko";
  return null;
}

function isNumericIterable(type) {
  return type.k === "arr" && type.e?.k === "kw" && type.e.v === "number";
}

function isBareNumericSource(type) {
  return asParts(type).every((part) => {
    if (part.k === "arr" && part.e?.k === "kw" && part.e.v === "number") return true;
    return part.k === "ref" && NUMERIC_TYPED_ARRAYS.has(part.v) && part.a.length === 0;
  });
}

function normalizeRaw(text) {
  return text.replace(/\s+/g, " ").replace(/\s*([|&])\s*/g, " $1 ").replace(/\s*([,<>()[\]?:])\s*/g, "$1").trim();
}
