import type { z } from 'zod';

/**
 * A typed IPC command: named arguments in, one value out.
 *
 * Argument keys are snake_case and match the Rust parameter names exactly —
 * Rust commands are declared with `#[tauri::command(rename_all = "snake_case")]`.
 * Commands returning `()` in Rust resolve to `null`.
 */
export interface CommandDef<
  Args extends z.ZodObject = z.ZodObject,
  Result extends z.ZodType = z.ZodType,
> {
  readonly args: Args;
  readonly result: Result;
  /** Build phase that implements the Rust side (documentation only). */
  readonly phase: number;
}

export type IpcContract = Readonly<Record<string, CommandDef>>;

export function command<Args extends z.ZodObject, Result extends z.ZodType>(
  args: Args,
  result: Result,
  phase: number,
): CommandDef<Args, Result> {
  return { args, result, phase };
}

export type CommandName<C extends IpcContract> = keyof C & string;
export type CommandArgs<C extends IpcContract, K extends CommandName<C>> = z.input<C[K]['args']>;
export type CommandResult<C extends IpcContract, K extends CommandName<C>> = z.output<
  C[K]['result']
>;
