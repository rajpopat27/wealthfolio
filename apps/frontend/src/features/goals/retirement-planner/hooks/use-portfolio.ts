import { useQuery } from "@tanstack/react-query";
import { getAccounts, getCurrentValuation, getHoldings } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { Holding } from "@/lib/types";

export function usePortfolioData(accountIds?: string[]) {
  const accountsQuery = useQuery({
    queryKey: [QueryKeys.ACCOUNTS],
    queryFn: () => getAccounts(),
    staleTime: 10 * 60 * 1000,
  });

  const accounts = accountsQuery.data ?? [];
  const allActiveAccounts = accounts.filter((a) => a.isActive && !a.isArchived);

  const activeAccountIds = (
    accountIds !== undefined ? allActiveAccounts.filter((a) => accountIds.includes(a.id)) : []
  ).map((a) => a.id);

  const currentValuationQuery = useQuery({
    queryKey: [QueryKeys.CURRENT_VALUATION, "fire", activeAccountIds],
    queryFn: () =>
      getCurrentValuation({
        filter: { type: "accounts", accountIds: activeAccountIds },
        includeAccounts: false,
      }),
    enabled: activeAccountIds.length > 0,
    staleTime: 5 * 60 * 1000,
  });

  const holdingsQuery = useQuery({
    queryKey: [QueryKeys.HOLDINGS, activeAccountIds],
    queryFn: async (): Promise<Holding[]> => {
      if (activeAccountIds.length === 0) return [];
      const perAccount = await Promise.all(
        activeAccountIds.map((id) => getHoldings({ type: "account", accountId: id })),
      );
      // Aggregate by symbol so drift analysis sees combined weights across all FIRE accounts.
      const bySymbol = new Map<string, Holding>();
      for (const holdings of perAccount) {
        for (const h of holdings) {
          const key = h.instrument?.symbol ?? h.id;
          const existing = bySymbol.get(key);
          if (existing) {
            existing.marketValue = {
              local: existing.marketValue.local + h.marketValue.local,
              base: existing.marketValue.base + h.marketValue.base,
            };
            existing.quantity = existing.quantity + h.quantity;
          } else {
            bySymbol.set(key, { ...h });
          }
        }
      }
      return Array.from(bySymbol.values());
    },
    enabled: activeAccountIds.length > 0,
    staleTime: 5 * 60 * 1000,
  });

  const totalValue = currentValuationQuery.data?.summary.totalValueBase ?? 0;

  const activeAccounts = accounts.filter((a) => activeAccountIds.includes(a.id));

  return {
    holdings: holdingsQuery.data ?? [],
    activeAccountIds,
    accounts,
    activeAccounts,
    totalValue,
    isLoading:
      accountsQuery.isLoading || currentValuationQuery.isLoading || holdingsQuery.isLoading,
    error: currentValuationQuery.error || holdingsQuery.error,
  };
}
