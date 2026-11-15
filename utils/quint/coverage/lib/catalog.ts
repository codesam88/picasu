import { join } from "node:path";
import { loadJson } from "./common.ts";

export interface EventEntry {
  name: string;
  status: string;
  description: string;
  actor: string;
  target: string;
  preconditions: string[];
  affected_state: string[];
  code_ref: string;
  effects: string[];
  faults: { fault: string; consequence: string; status: string }[];
  notes: string;
  variants?: { name: string; code_ref: string; notes: string }[];
}

export interface EventsDoc {
  version: number;
  description: string;
  events: EventEntry[];
}

export interface GoalEntry {
  id: string;
  decision: string;
  title: string;
  description: string;
  anchors: { kind: string; ref: string }[];
  assumptions?: string[];
  notes?: string;
}

export interface GoalsDoc {
  version?: number;
  description: string;
  goals: GoalEntry[];
}

export interface RequirementEntry {
  id: string;
  title: string;
  statement: string;
  derived_from: string[];
  owner: string;
  gate: string;
  status: string;
  implementation: string;
  tests: string[];
  limits?: string;
}

export interface RequirementsDoc {
  version?: number;
  description: string;
  requirements: RequirementEntry[];
}

export interface AssumptionEntry {
  id: string;
  title: string;
  owner: string;
  status: string;
  status_note?: string;
}

export interface AssumptionsDoc {
  version?: number;
  description: string;
  assumptions: AssumptionEntry[];
}

export interface ScopeEntry {
  id: string;
  title: string;
  bound: string;
  why_safe: string;
  lifting: string;
  provisional?: boolean;
}

export interface ScopeDoc {
  version?: number;
  description: string;
  modeled_scope: string;
  scopes: ScopeEntry[];
}

export interface OperationProperty {
  model: string;
  test?: string;
}

export interface OperationEntry {
  id: string;
  name: string;
  events: string[];
  reads: string[];
  writes: string[];
  guards: string[];
  status: string;
  nodes: string[];
  notes: string;
  properties?: OperationProperty[];
}

export interface OperationsDoc {
  version: number;
  description: string;
  operations: OperationEntry[];
}

export function loadEventsDoc(base: string): EventsDoc {
  return loadJson(join(base, "events.json")) as EventsDoc;
}

export function loadGoalsDoc(base: string): GoalsDoc {
  return loadJson(join(base, "goals.json")) as GoalsDoc;
}

export function loadRequirementsDoc(base: string): RequirementsDoc {
  return loadJson(join(base, "requirements.json")) as RequirementsDoc;
}

export function loadAssumptionsDoc(base: string): AssumptionsDoc {
  return loadJson(join(base, "assumptions.json")) as AssumptionsDoc;
}

export function loadScopeDoc(base: string): ScopeDoc {
  return loadJson(join(base, "scope.json")) as ScopeDoc;
}

export function loadOperationsDoc(base: string): OperationsDoc {
  return loadJson(join(base, "operations.json")) as OperationsDoc;
}

export function loadSchema(base: string): any {
  return loadJson(join(base, "catalog.schema.json"));
}

// Avoid unused import lint
