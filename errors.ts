/**
 * Structured Git Error Handling
 *
 * This module provides TypeScript types and helpers for working with structured
 * errors from the native Git module.
 *
 * ## Background
 *
 * When the `structured_errors` feature flag is enabled (via LIMINAL_FEATURE_FLAGS),
 * the native module returns errors with JSON-serialized structured data in the
 * error message field. This is necessary because napi-rs 3.3 doesn't support
 * attaching arbitrary properties to error objects.
 *
 * ## Usage
 *
 * ```typescript
 * import { GitService } from './index.js';
 * import { parseStructuredGitError, isFileNotFoundError } from './errors.js';
 *
 * const gitService = new GitService();
 *
 * try {
 *   await gitService.commitFile(path, message, user, email);
 * } catch (err) {
 *   const structured = parseStructuredGitError(err);
 *
 *   if (structured && isFileNotFoundError(structured)) {
 *     console.log(`File not found: ${structured.details.path}`);
 *     if (structured.retriable) {
 *       // Safe to retry
 *     }
 *   }
 * }
 * ```
 */

/**
 * Structured error codes returned by the native Git module
 *
 * These codes are stable and can be relied upon for programmatic error handling.
 */
export type GitErrorCode =
  // Repository errors
  | 'REPOSITORY_NOT_FOUND'
  | 'REPOSITORY_CORRUPTED'
  | 'INVALID_REPOSITORY'
  // File errors
  | 'FILE_NOT_FOUND'
  | 'FILE_NOT_IN_REPOSITORY'
  | 'PATH_TRAVERSAL'
  // Operation errors
  | 'NOTHING_TO_COMMIT'
  | 'MERGE_CONFLICT'
  | 'UNCOMMITTED_CHANGES'
  | 'UNSTAGED_CHANGES_WOULD_BE_LOST'
  | 'DETACHED_HEAD'
  // Branch errors
  | 'BRANCH_NOT_FOUND'
  | 'BRANCH_ALREADY_EXISTS'
  | 'CANNOT_DELETE_CURRENT_BRANCH'
  | 'BRANCH_NOT_MERGED'
  // Tag errors
  | 'TAG_NOT_FOUND'
  | 'TAG_ALREADY_EXISTS'
  // Validation errors
  | 'INVALID_PATH'
  | 'INVALID_COMMIT_HASH'
  | 'INVALID_BRANCH_NAME'
  | 'INVALID_TAG_NAME'
  // System errors
  | 'IO_ERROR'
  | 'GIT_OPERATION_FAILURE'
  // Config errors
  | 'CONFIG_MISSING';

/**
 * Base structured error interface
 *
 * All structured errors from the native module conform to this shape.
 */
export interface StructuredGitError<TDetails = Record<string, unknown>> {
  /** Machine-readable error code */
  code: GitErrorCode;
  /** Human-readable error message */
  message: string;
  /** Whether this error is safe to retry */
  retriable: boolean;
  /** Additional structured details specific to the error type */
  details: TDetails;
}

/**
 * Error detail types for specific error codes
 */

export interface RepositoryNotFoundDetails {
  path: string;
}

export interface RepositoryCorruptedDetails {
  path: string;
  errorDetails: string;
}

export interface InvalidRepositoryDetails {
  path: string;
}

export interface FileNotFoundDetails {
  path: string;
}

export interface FileNotInRepositoryDetails {
  path: string;
}

export interface PathTraversalDetails {
  attemptedPath: string;
}

export interface MergeConflictDetails {
  files: string[];
}

export interface UncommittedChangesDetails {
  count: number;
}

export interface UnstagedChangesWouldBeLostDetails {
  files: string[];
  count: number;
}

export interface ConfigMissingDetails {
  key: string;
  triedLocations: string[];
}

export interface BranchNotFoundDetails {
  name: string;
}

export interface BranchAlreadyExistsDetails {
  name: string;
}

export interface CannotDeleteCurrentBranchDetails {
  name: string;
}

export interface BranchNotMergedDetails {
  name: string;
  commitsAhead: number;
}

export interface TagNotFoundDetails {
  name: string;
}

export interface TagAlreadyExistsDetails {
  name: string;
}

export interface InvalidPathDetails {
  path: string;
  reason: string;
}

export interface InvalidCommitHashDetails {
  hash: string;
}

export interface InvalidBranchNameDetails {
  name: string;
}

export interface InvalidTagNameDetails {
  name: string;
}

export interface IoErrorDetails {
  operation: string;
  error: string;
}

export interface GitOperationFailureDetails {
  operation: string;
  class: number;
  code: number;
  gitMessage: string;
}

/**
 * Typed structured error unions for specific error codes
 */

export type FileNotFoundError = StructuredGitError<FileNotFoundDetails>;
export type BranchNotFoundError = StructuredGitError<BranchNotFoundDetails>;
export type MergeConflictError = StructuredGitError<MergeConflictDetails>;
export type InvalidPathError = StructuredGitError<InvalidPathDetails>;
export type UnstagedChangesWouldBeLostError = StructuredGitError<UnstagedChangesWouldBeLostDetails>;
export type ConfigMissingError = StructuredGitError<ConfigMissingDetails>;

/**
 * Parse a structured Git error from a caught exception
 *
 * This function safely attempts to parse JSON from the error message.
 * If parsing fails or the error isn't structured, returns null.
 *
 * @param error - The caught error from a Git operation
 * @returns Parsed structured error or null if not a structured error
 *
 * @example
 * ```typescript
 * try {
 *   await gitService.commitFile(path, message, user, email);
 * } catch (err) {
 *   const structured = parseStructuredGitError(err);
 *   if (structured) {
 *     console.log(`Error ${structured.code}: ${structured.message}`);
 *   } else {
 *     // Legacy unstructured error
 *     console.error(err);
 *   }
 * }
 * ```
 */
export function parseStructuredGitError(
  error: unknown
): StructuredGitError | null {
  // Check if it's an Error object with a message
  if (!error || typeof error !== 'object' || !('message' in error)) {
    return null;
  }

  const message = (error as Error).message;

  // Attempt to parse JSON from the message
  try {
    const parsed = JSON.parse(message) as unknown;

    // Validate it has the expected structure
    if (
      parsed &&
      typeof parsed === 'object' &&
      'code' in parsed &&
      'message' in parsed &&
      'retriable' in parsed &&
      'details' in parsed &&
      typeof (parsed as { code: unknown }).code === 'string' &&
      typeof (parsed as { message: unknown }).message === 'string' &&
      typeof (parsed as { retriable: unknown }).retriable === 'boolean' &&
      typeof (parsed as { details: unknown }).details === 'object'
    ) {
      return parsed as StructuredGitError;
    }
  } catch {
    // Not JSON or invalid structure - return null
    return null;
  }

  return null;
}

/**
 * Type guard: Check if error is a file-not-found error
 */
export function isFileNotFoundError(
  error: StructuredGitError<any>
): error is FileNotFoundError {
  return error.code === 'FILE_NOT_FOUND';
}

/**
 * Type guard: Check if error is a branch-not-found error
 */
export function isBranchNotFoundError(
  error: StructuredGitError<any>
): error is BranchNotFoundError {
  return error.code === 'BRANCH_NOT_FOUND';
}

/**
 * Type guard: Check if error is a merge conflict error
 */
export function isMergeConflictError(
  error: StructuredGitError<any>
): error is MergeConflictError {
  return error.code === 'MERGE_CONFLICT';
}

/**
 * Type guard: Check if error is an invalid path error
 */
export function isInvalidPathError(
  error: StructuredGitError<any>
): error is InvalidPathError {
  return error.code === 'INVALID_PATH';
}

/**
 * Type guard: Check if error is an unstaged-changes-would-be-lost error
 */
export function isUnstagedChangesWouldBeLostError(
  error: StructuredGitError<any>
): error is UnstagedChangesWouldBeLostError {
  return error.code === 'UNSTAGED_CHANGES_WOULD_BE_LOST';
}

/**
 * Type guard: Check if error is a config-missing error
 */
export function isConfigMissingError(
  error: StructuredGitError<any>
): error is ConfigMissingError {
  return error.code === 'CONFIG_MISSING';
}

/**
 * Type guard: Check if error is a repository error (any repository-related error)
 */
export function isRepositoryError(error: StructuredGitError<any>): boolean {
  return (
    error.code === 'REPOSITORY_NOT_FOUND' ||
    error.code === 'REPOSITORY_CORRUPTED' ||
    error.code === 'INVALID_REPOSITORY'
  );
}

/**
 * Type guard: Check if error is a file error (any file-related error)
 */
export function isFileError(error: StructuredGitError<any>): boolean {
  return (
    error.code === 'FILE_NOT_FOUND' ||
    error.code === 'FILE_NOT_IN_REPOSITORY' ||
    error.code === 'PATH_TRAVERSAL'
  );
}

/**
 * Type guard: Check if error is a branch error (any branch-related error)
 */
export function isBranchError(error: StructuredGitError<any>): boolean {
  return (
    error.code === 'BRANCH_NOT_FOUND' ||
    error.code === 'BRANCH_ALREADY_EXISTS' ||
    error.code === 'CANNOT_DELETE_CURRENT_BRANCH' ||
    error.code === 'BRANCH_NOT_MERGED'
  );
}

/**
 * Type guard: Check if error is a tag error (any tag-related error)
 */
export function isTagError(error: StructuredGitError<any>): boolean {
  return (
    error.code === 'TAG_NOT_FOUND' ||
    error.code === 'TAG_ALREADY_EXISTS'
  );
}

/**
 * Type guard: Check if error is retryable
 */
export function isRetryableError(error: StructuredGitError<any>): boolean {
  return error.retriable;
}
