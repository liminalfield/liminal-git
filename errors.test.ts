/**
 * Tests for structured error parsing
 *
 * These tests verify that the parseStructuredGitError helper correctly handles
 * both structured JSON errors and legacy string errors.
 */

import { describe, it, expect } from '@jest/globals';
import {
  parseStructuredGitError,
  isFileNotFoundError,
  isBranchNotFoundError,
  isMergeConflictError,
  isUnstagedChangesWouldBeLostError,
  isConfigMissingError,
  isRepositoryError,
  isFileError,
  isBranchError,
  isTagError,
  isRetryableError,
  type StructuredGitError,
} from './errors';

describe('parseStructuredGitError', () => {
  it('should parse valid structured error JSON', () => {
    const errorMessage = JSON.stringify({
      code: 'FILE_NOT_FOUND',
      message: 'File not found: /test/file.txt',
      retriable: false,
      details: { path: '/test/file.txt' },
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    expect(parsed).not.toBeNull();
    expect(parsed?.code).toBe('FILE_NOT_FOUND');
    expect(parsed?.message).toBe('File not found: /test/file.txt');
    expect(parsed?.retriable).toBe(false);
    expect(parsed?.details).toEqual({ path: '/test/file.txt' });
  });

  it('should return null for non-JSON error messages', () => {
    const error = new Error('Plain error message');
    const parsed = parseStructuredGitError(error);

    expect(parsed).toBeNull();
  });

  it('should return null for invalid JSON', () => {
    const error = new Error('{ invalid json }');
    const parsed = parseStructuredGitError(error);

    expect(parsed).toBeNull();
  });

  it('should return null for JSON without required fields', () => {
    const errorMessage = JSON.stringify({
      code: 'FILE_NOT_FOUND',
      message: 'File not found',
      // Missing retriable and details
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    expect(parsed).toBeNull();
  });

  it('should return null for non-Error objects', () => {
    expect(parseStructuredGitError(null)).toBeNull();
    expect(parseStructuredGitError(undefined)).toBeNull();
    expect(parseStructuredGitError('string error')).toBeNull();
    expect(parseStructuredGitError(42)).toBeNull();
  });

  it('should parse error with empty details object', () => {
    const errorMessage = JSON.stringify({
      code: 'NOTHING_TO_COMMIT',
      message: 'Nothing to commit',
      retriable: false,
      details: {},
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    expect(parsed).not.toBeNull();
    expect(parsed?.code).toBe('NOTHING_TO_COMMIT');
    expect(parsed?.details).toEqual({});
  });

  it('should parse error with complex details', () => {
    const errorMessage = JSON.stringify({
      code: 'MERGE_CONFLICT',
      message: 'Merge conflict in 2 file(s)',
      retriable: false,
      details: {
        files: ['file1.txt', 'file2.txt'],
      },
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    expect(parsed).not.toBeNull();
    expect(parsed?.code).toBe('MERGE_CONFLICT');
    expect(parsed?.details).toEqual({
      files: ['file1.txt', 'file2.txt'],
    });
  });
});

describe('Type guards', () => {
  const createStructuredError = (
    code: string,
    retriable = false
  ): StructuredGitError => ({
    code: code as any,
    message: `Test ${code}`,
    retriable,
    details: {},
  });

  describe('Specific error type guards', () => {
    it('isFileNotFoundError should identify FILE_NOT_FOUND', () => {
      const error = createStructuredError('FILE_NOT_FOUND');
      expect(isFileNotFoundError(error)).toBe(true);

      const otherError = createStructuredError('BRANCH_NOT_FOUND');
      expect(isFileNotFoundError(otherError)).toBe(false);
    });

    it('isBranchNotFoundError should identify BRANCH_NOT_FOUND', () => {
      const error = createStructuredError('BRANCH_NOT_FOUND');
      expect(isBranchNotFoundError(error)).toBe(true);

      const otherError = createStructuredError('FILE_NOT_FOUND');
      expect(isBranchNotFoundError(otherError)).toBe(false);
    });

    it('isMergeConflictError should identify MERGE_CONFLICT', () => {
      const error = createStructuredError('MERGE_CONFLICT');
      expect(isMergeConflictError(error)).toBe(true);

      const otherError = createStructuredError('FILE_NOT_FOUND');
      expect(isMergeConflictError(otherError)).toBe(false);
    });

    it('isUnstagedChangesWouldBeLostError should identify UNSTAGED_CHANGES_WOULD_BE_LOST', () => {
      const error = createStructuredError('UNSTAGED_CHANGES_WOULD_BE_LOST');
      expect(isUnstagedChangesWouldBeLostError(error)).toBe(true);

      const otherError = createStructuredError('FILE_NOT_FOUND');
      expect(isUnstagedChangesWouldBeLostError(otherError)).toBe(false);
    });

    it('isConfigMissingError should identify CONFIG_MISSING', () => {
      const error = createStructuredError('CONFIG_MISSING');
      expect(isConfigMissingError(error)).toBe(true);

      const otherError = createStructuredError('FILE_NOT_FOUND');
      expect(isConfigMissingError(otherError)).toBe(false);
    });
  });

  describe('Category type guards', () => {
    it('isRepositoryError should identify repository errors', () => {
      expect(isRepositoryError(createStructuredError('REPOSITORY_NOT_FOUND'))).toBe(true);
      expect(isRepositoryError(createStructuredError('REPOSITORY_CORRUPTED'))).toBe(true);
      expect(isRepositoryError(createStructuredError('INVALID_REPOSITORY'))).toBe(true);
      expect(isRepositoryError(createStructuredError('FILE_NOT_FOUND'))).toBe(false);
    });

    it('isFileError should identify file errors', () => {
      expect(isFileError(createStructuredError('FILE_NOT_FOUND'))).toBe(true);
      expect(isFileError(createStructuredError('FILE_NOT_IN_REPOSITORY'))).toBe(true);
      expect(isFileError(createStructuredError('PATH_TRAVERSAL'))).toBe(true);
      expect(isFileError(createStructuredError('BRANCH_NOT_FOUND'))).toBe(false);
    });

    it('isBranchError should identify branch errors', () => {
      expect(isBranchError(createStructuredError('BRANCH_NOT_FOUND'))).toBe(true);
      expect(isBranchError(createStructuredError('BRANCH_ALREADY_EXISTS'))).toBe(true);
      expect(isBranchError(createStructuredError('CANNOT_DELETE_CURRENT_BRANCH'))).toBe(true);
      expect(isBranchError(createStructuredError('BRANCH_NOT_MERGED'))).toBe(true);
      expect(isBranchError(createStructuredError('FILE_NOT_FOUND'))).toBe(false);
    });

    it('isTagError should identify tag errors', () => {
      expect(isTagError(createStructuredError('TAG_NOT_FOUND'))).toBe(true);
      expect(isTagError(createStructuredError('TAG_ALREADY_EXISTS'))).toBe(true);
      expect(isTagError(createStructuredError('BRANCH_NOT_FOUND'))).toBe(false);
    });
  });

  describe('isRetryableError', () => {
    it('should return true for retryable errors', () => {
      const error = createStructuredError('IO_ERROR', true);
      expect(isRetryableError(error)).toBe(true);
    });

    it('should return false for non-retryable errors', () => {
      const error = createStructuredError('FILE_NOT_FOUND', false);
      expect(isRetryableError(error)).toBe(false);
    });
  });
});

describe('Type narrowing with type guards', () => {
  it('should narrow types correctly with isFileNotFoundError', () => {
    const errorMessage = JSON.stringify({
      code: 'FILE_NOT_FOUND',
      message: 'File not found: /test/file.txt',
      retriable: false,
      details: { path: '/test/file.txt' },
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    if (parsed && isFileNotFoundError(parsed)) {
      // TypeScript should narrow to FileNotFoundError
      expect(parsed.details.path).toBe('/test/file.txt');
    } else {
      fail('Should have parsed as FileNotFoundError');
    }
  });

  it('should narrow types correctly with isMergeConflictError', () => {
    const errorMessage = JSON.stringify({
      code: 'MERGE_CONFLICT',
      message: 'Merge conflict in 2 file(s)',
      retriable: false,
      details: { files: ['file1.txt', 'file2.txt'] },
    });

    const error = new Error(errorMessage);
    const parsed = parseStructuredGitError(error);

    if (parsed && isMergeConflictError(parsed)) {
      // TypeScript should narrow to MergeConflictError
      expect(parsed.details.files).toEqual(['file1.txt', 'file2.txt']);
      expect(parsed.details.files.length).toBe(2);
    } else {
      fail('Should have parsed as MergeConflictError');
    }
  });
});
