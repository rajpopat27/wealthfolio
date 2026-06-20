import { describe, expect, it } from "vitest";
import { QuoteMode } from "./constants";
import { importActivitySchema, importMappingSchema, newAccountSchema } from "./schemas";

describe("schemas", () => {
  describe("importMappingSchema", () => {
    it("should accept valid quoteMode values in symbolMappingMeta", () => {
      const validMapping = {
        accountId: "test-account",
        symbolMappingMeta: {
          AAPL: {
            quoteMode: QuoteMode.MARKET,
          },
          CUSTOM: {
            quoteMode: QuoteMode.MANUAL,
          },
        },
      };

      const result = importMappingSchema.safeParse(validMapping);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.symbolMappingMeta?.AAPL.quoteMode).toBe(QuoteMode.MARKET);
        expect(result.data.symbolMappingMeta?.CUSTOM.quoteMode).toBe(QuoteMode.MANUAL);
      }
    });

    it("should reject invalid quoteMode values in symbolMappingMeta", () => {
      const invalidMapping = {
        accountId: "test-account",
        symbolMappingMeta: {
          AAPL: {
            quoteMode: "INVALID_MODE",
          },
        },
      };

      const result = importMappingSchema.safeParse(invalidMapping);
      expect(result.success).toBe(false);
    });

    it("should allow missing quoteMode in symbolMappingMeta", () => {
      const mappingWithoutQuoteMode = {
        accountId: "test-account",
        symbolMappingMeta: {
          AAPL: {
            exchangeMic: "XNAS",
            symbolName: "Apple Inc.",
          },
        },
      };

      const result = importMappingSchema.safeParse(mappingWithoutQuoteMode);
      expect(result.success).toBe(true);
    });
  });

  describe("importActivitySchema", () => {
    it("should accept valid quoteMode values", () => {
      const marketActivity = {
        accountId: "account-1",
        activityType: "BUY",
        date: "2024-01-15",
        symbol: "AAPL",
        unitPrice: 150.5,
        isDraft: false,
        quoteMode: QuoteMode.MARKET,
      };

      let result = importActivitySchema.safeParse(marketActivity);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.quoteMode).toBe(QuoteMode.MARKET);
      }

      const manualActivity = {
        accountId: "account-1",
        activityType: "BUY",
        date: "2024-01-15",
        symbol: "CUSTOM",
        unitPrice: 100.0,
        isDraft: false,
        quoteMode: QuoteMode.MANUAL,
      };

      result = importActivitySchema.safeParse(manualActivity);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.quoteMode).toBe(QuoteMode.MANUAL);
      }
    });

    it("should reject invalid quoteMode values", () => {
      const invalidActivity = {
        accountId: "account-1",
        activityType: "BUY",
        date: "2024-01-15",
        symbol: "AAPL",
        unitPrice: 150.5,
        isDraft: false,
        quoteMode: "INVALID_MODE",
      };

      const result = importActivitySchema.safeParse(invalidActivity);
      expect(result.success).toBe(false);
    });

    it("should allow missing quoteMode", () => {
      const activityWithoutQuoteMode = {
        accountId: "account-1",
        activityType: "BUY",
        date: "2024-01-15",
        symbol: "AAPL",
        unitPrice: 150.5,
        isDraft: false,
      };

      const result = importActivitySchema.safeParse(activityWithoutQuoteMode);
      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.quoteMode).toBeUndefined();
      }
    });

    it("preserves clientImportId for import row correlation", () => {
      const result = importActivitySchema.safeParse({
        accountId: "account-1",
        clientImportId: "import-addon:2:0",
        activityType: "DEPOSIT",
        date: "2026-06-20",
        amount: 123.45,
        currency: "INR",
        isDraft: false,
      });

      expect(result.success).toBe(true);
      if (result.success) {
        expect(result.data.clientImportId).toBe("import-addon:2:0");
      }
    });
  });

  describe("newAccountSchema", () => {
    it("does not accept import row correlation fields", () => {
      const result = newAccountSchema.safeParse({
        clientImportId: "wrong-schema",
        name: "Brokerage",
        accountType: "SECURITIES",
        currency: "INR",
      });

      expect(result.success).toBe(true);
      if (result.success) {
        expect("clientImportId" in result.data).toBe(false);
      }
    });
  });
});
