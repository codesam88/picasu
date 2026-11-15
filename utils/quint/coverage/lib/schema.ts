// Minimal draft-07 subset interpreter. Supports: type, required, properties,
// additionalProperties, items, enum, minItems, and local $ref (#/definitions/X).
// Fails loudly on any keyword it does not implement.

const ALLOWED_KEYS = new Set([
  "type",
  "required",
  "properties",
  "additionalProperties",
  "items",
  "enum",
  "minItems",
  "$ref",
  "title",
  "description",
  "$schema",
  "definitions",
]);

export type JsonSchema = any;

export class SchemaError extends Error {}

function isPlainObject(v: any): v is Record<string, any> {
  return v !== null && typeof v === "object" && !Array.isArray(v);
}

function checkUnsupported(schema: any, path: string): void {
  if (!isPlainObject(schema)) return;
  for (const key of Object.keys(schema)) {
    if (!ALLOWED_KEYS.has(key)) {
      throw new SchemaError(
        `schema uses unsupported keyword '${key}' at ${path}`,
      );
    }
  }
}

function resolveRef(ref: string, root: any): any {
  if (!ref.startsWith("#/")) throw new SchemaError(`unsupported $ref '${ref}'`);
  const parts = ref.slice(2).split("/");
  let cur: any = root;
  for (const p of parts) {
    if (cur == null || typeof cur !== "object")
      throw new SchemaError(`unresolved $ref '${ref}'`);
    cur = cur[p];
  }
  if (cur === undefined) throw new SchemaError(`unresolved $ref '${ref}'`);
  return cur;
}

export function validate(
  value: any,
  schema: any,
  root: any = schema,
  path: string = "$",
): string[] {
  const errors: string[] = [];
  if (schema === null || schema === undefined) return errors;
  if (typeof schema === "boolean") {
    if (schema === false) errors.push(`${path}: schema is false`);
    return errors;
  }
  if (!isPlainObject(schema)) {
    throw new SchemaError(`schema must be an object at ${path}`);
  }
  checkUnsupported(schema, path);

  // $ref
  if (typeof schema.$ref === "string") {
    let resolved: any;
    try {
      resolved = resolveRef(schema.$ref, root);
    } catch (e: any) {
      throw e;
    }
    // merge? In draft-07 $ref replaces. We'll validate against resolved only,
    // but still apply sibling keywords? Usually $ref with siblings is not
    // allowed in draft-07. Here we ignore siblings when $ref present.
    return validate(value, resolved, root, path);
  }

  // type
  if (schema.type !== undefined) {
    const t = schema.type;
    const ok = checkType(value, t);
    if (!ok) errors.push(`${path}: expected type ${t}, got ${typeOf(value)}`);
  }

  // enum
  if (Array.isArray(schema.enum)) {
    if (!schema.enum.some((v: any) => deepEqual(v, value))) {
      errors.push(`${path}: value not in enum`);
    }
  }

  // minItems
  if (typeof schema.minItems === "number") {
    if (!Array.isArray(value) || value.length < schema.minItems) {
      errors.push(`${path}: must have at least ${schema.minItems} items`);
    }
  }

  // required
  if (Array.isArray(schema.required)) {
    if (!isPlainObject(value)) {
      // type check already recorded; skip
    } else {
      for (const key of schema.required) {
        if (!(key in value))
          errors.push(`${path}: missing required field '${key}'`);
      }
    }
  }

  // properties
  if (isPlainObject(schema.properties)) {
    if (isPlainObject(value)) {
      for (const key of Object.keys(schema.properties)) {
        if (key in value) {
          errors.push(
            ...validate(
              value[key],
              schema.properties[key],
              root,
              `${path}.${key}`,
            ),
          );
        }
      }
    }
  }

  // additionalProperties
  if (schema.additionalProperties !== undefined) {
    if (schema.additionalProperties === false) {
      if (isPlainObject(value) && isPlainObject(schema.properties)) {
        for (const key of Object.keys(value)) {
          if (!(key in schema.properties)) {
            errors.push(`${path}: additional property '${key}' not allowed`);
          }
        }
      }
    } else if (isPlainObject(schema.additionalProperties)) {
      if (isPlainObject(value)) {
        const props = isPlainObject(schema.properties) ? schema.properties : {};
        for (const key of Object.keys(value)) {
          if (!(key in props)) {
            errors.push(
              ...validate(
                value[key],
                schema.additionalProperties,
                root,
                `${path}.${key}`,
              ),
            );
          }
        }
      }
    } else if (typeof schema.additionalProperties !== "boolean") {
      throw new SchemaError(`invalid additionalProperties at ${path}`);
    }
  }

  // items
  if (schema.items !== undefined) {
    if (Array.isArray(value)) {
      const itemSchema = schema.items;
      value.forEach((item, idx) => {
        errors.push(...validate(item, itemSchema, root, `${path}[${idx}]`));
      });
    }
  }

  return errors;
}

function checkType(value: any, t: any): boolean {
  if (Array.isArray(t)) return t.some((x) => checkType(value, x));
  switch (t) {
    case "string":
      return typeof value === "string";
    case "number":
      return typeof value === "number";
    case "integer":
      return typeof value === "number" && Number.isInteger(value);
    case "boolean":
      return typeof value === "boolean";
    case "array":
      return Array.isArray(value);
    case "object":
      return isPlainObject(value);
    case "null":
      return value === null;
    default:
      throw new SchemaError(`unsupported type '${t}'`);
  }
}

function typeOf(value: any): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function deepEqual(a: any, b: any): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
