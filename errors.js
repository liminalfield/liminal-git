"use strict";
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
Object.defineProperty(exports, "__esModule", { value: true });
exports.parseStructuredGitError = parseStructuredGitError;
exports.isFileNotFoundError = isFileNotFoundError;
exports.isBranchNotFoundError = isBranchNotFoundError;
exports.isMergeConflictError = isMergeConflictError;
exports.isInvalidPathError = isInvalidPathError;
exports.isUnstagedChangesWouldBeLostError = isUnstagedChangesWouldBeLostError;
exports.isConfigMissingError = isConfigMissingError;
exports.isRepositoryError = isRepositoryError;
exports.isFileError = isFileError;
exports.isBranchError = isBranchError;
exports.isTagError = isTagError;
exports.isRetryableError = isRetryableError;
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
function parseStructuredGitError(error) {
    // Check if it's an Error object with a message
    if (!error || typeof error !== 'object' || !('message' in error)) {
        return null;
    }
    const message = error.message;
    // Attempt to parse JSON from the message
    try {
        const parsed = JSON.parse(message);
        // Validate it has the expected structure
        if (parsed &&
            typeof parsed === 'object' &&
            'code' in parsed &&
            'message' in parsed &&
            'retriable' in parsed &&
            'details' in parsed &&
            typeof parsed.code === 'string' &&
            typeof parsed.message === 'string' &&
            typeof parsed.retriable === 'boolean' &&
            typeof parsed.details === 'object') {
            return parsed;
        }
    }
    catch {
        // Not JSON or invalid structure - return null
        return null;
    }
    return null;
}
/**
 * Type guard: Check if error is a file-not-found error
 */
function isFileNotFoundError(error) {
    return error.code === 'FILE_NOT_FOUND';
}
/**
 * Type guard: Check if error is a branch-not-found error
 */
function isBranchNotFoundError(error) {
    return error.code === 'BRANCH_NOT_FOUND';
}
/**
 * Type guard: Check if error is a merge conflict error
 */
function isMergeConflictError(error) {
    return error.code === 'MERGE_CONFLICT';
}
/**
 * Type guard: Check if error is an invalid path error
 */
function isInvalidPathError(error) {
    return error.code === 'INVALID_PATH';
}
/**
 * Type guard: Check if error is an unstaged-changes-would-be-lost error
 */
function isUnstagedChangesWouldBeLostError(error) {
    return error.code === 'UNSTAGED_CHANGES_WOULD_BE_LOST';
}
/**
 * Type guard: Check if error is a config-missing error
 */
function isConfigMissingError(error) {
    return error.code === 'CONFIG_MISSING';
}
/**
 * Type guard: Check if error is a repository error (any repository-related error)
 */
function isRepositoryError(error) {
    return (error.code === 'REPOSITORY_NOT_FOUND' ||
        error.code === 'REPOSITORY_CORRUPTED' ||
        error.code === 'INVALID_REPOSITORY');
}
/**
 * Type guard: Check if error is a file error (any file-related error)
 */
function isFileError(error) {
    return (error.code === 'FILE_NOT_FOUND' ||
        error.code === 'FILE_NOT_IN_REPOSITORY' ||
        error.code === 'PATH_TRAVERSAL');
}
/**
 * Type guard: Check if error is a branch error (any branch-related error)
 */
function isBranchError(error) {
    return (error.code === 'BRANCH_NOT_FOUND' ||
        error.code === 'BRANCH_ALREADY_EXISTS' ||
        error.code === 'CANNOT_DELETE_CURRENT_BRANCH' ||
        error.code === 'BRANCH_NOT_MERGED');
}
/**
 * Type guard: Check if error is a tag error (any tag-related error)
 */
function isTagError(error) {
    return (error.code === 'TAG_NOT_FOUND' ||
        error.code === 'TAG_ALREADY_EXISTS');
}
/**
 * Type guard: Check if error is retryable
 */
function isRetryableError(error) {
    return error.retriable;
}
