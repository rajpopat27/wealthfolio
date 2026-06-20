import { vi, describe, it, expect } from "vitest";
import { createSDKHostAPIBridge, type InternalHostAPI } from "./type-bridge";
import type { TaxonomyWithCategories } from "@/lib/types";

describe("Addon Type Bridge", () => {
  describe("createSDKHostAPIBridge", () => {
    it("should create logger with addon prefix", () => {
      // Mock the internal API logger functions
      const mockLogError = vi.fn();
      const mockLogInfo = vi.fn();
      const mockLogWarn = vi.fn();
      const mockLogTrace = vi.fn();
      const mockLogDebug = vi.fn();

      // Create a minimal mock internal API with just the logger functions
      const mockInternalAPI: Partial<InternalHostAPI> = {
        logError: mockLogError,
        logInfo: mockLogInfo,
        logWarn: mockLogWarn,
        logTrace: mockLogTrace,
        logDebug: mockLogDebug,
      };

      // Create the SDK bridge with a test addon ID
      const sdkAPI = createSDKHostAPIBridge(mockInternalAPI as InternalHostAPI, "test-addon");

      // Test that logger methods add the addon prefix
      sdkAPI.logger.error("test error message");
      sdkAPI.logger.info("test info message");
      sdkAPI.logger.warn("test warning message");
      sdkAPI.logger.trace("test trace message");
      sdkAPI.logger.debug("test debug message");

      // Verify the logger functions were called with prefixed messages
      expect(mockLogError).toHaveBeenCalledWith("[test-addon] test error message");
      expect(mockLogInfo).toHaveBeenCalledWith("[test-addon] test info message");
      expect(mockLogWarn).toHaveBeenCalledWith("[test-addon] test warning message");
      expect(mockLogTrace).toHaveBeenCalledWith("[test-addon] test trace message");
      expect(mockLogDebug).toHaveBeenCalledWith("[test-addon] test debug message");
    });

    it("should use default addon ID when none provided", () => {
      const mockLogInfo = vi.fn();

      const mockInternalAPI: Partial<InternalHostAPI> = {
        logInfo: mockLogInfo,
      };

      // Create the SDK bridge without addon ID
      const sdkAPI = createSDKHostAPIBridge(mockInternalAPI as InternalHostAPI);

      sdkAPI.logger.info("test message");

      // Should use default addon ID
      expect(mockLogInfo).toHaveBeenCalledWith("[unknown-addon] test message");
    });

    it("should handle empty addon ID", () => {
      const mockLogInfo = vi.fn();

      const mockInternalAPI: Partial<InternalHostAPI> = {
        logInfo: mockLogInfo,
      };

      // Create the SDK bridge with empty addon ID
      const sdkAPI = createSDKHostAPIBridge(mockInternalAPI as InternalHostAPI, "");

      sdkAPI.logger.info("test message");

      // Should fallback to default addon ID for empty string
      expect(mockLogInfo).toHaveBeenCalledWith("[unknown-addon] test message");
    });

    it("maps activity taxonomy categories using existing Wealthfolio taxonomy IDs", async () => {
      const getTaxonomy = vi.fn(async (id: string): Promise<TaxonomyWithCategories | null> => ({
        taxonomy: {
          id,
          name: id,
          color: "#000000",
          isSystem: true,
          isSingleSelect: true,
          sortOrder: 0,
          createdAt: "2026-06-20T00:00:00Z",
          updatedAt: "2026-06-20T00:00:00Z",
          scope: "activity",
        },
        categories: [
          {
            id: `${id}-cat`,
            taxonomyId: id,
            parentId: null,
            name: `${id} Category`,
            key: `${id}_category`,
            color: "#000000",
            sortOrder: 0,
            createdAt: "2026-06-20T00:00:00Z",
            updatedAt: "2026-06-20T00:00:00Z",
          },
        ],
      }));

      const sdkAPI = createSDKHostAPIBridge({
        getTaxonomy,
      } as Partial<InternalHostAPI> as InternalHostAPI);

      const categories = await sdkAPI.spending.categories.list();

      expect(getTaxonomy).toHaveBeenCalledWith("spending_categories");
      expect(getTaxonomy).toHaveBeenCalledWith("income_sources");
      expect(getTaxonomy).toHaveBeenCalledWith("savings_categories");
      expect(categories.income[0]).toMatchObject({
        id: "income_sources-cat",
        taxonomyId: "income_sources",
      });
    });
  });
});
