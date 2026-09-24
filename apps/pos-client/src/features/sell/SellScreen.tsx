import type { Session } from '@pos/shared';
import { useAppInfo } from '../../ipc/queries';
import { CafeSell } from './CafeSell';
import { QuickSale } from './QuickSale';
import { RestaurantSell } from './RestaurantSell';

/** The selling layout follows the business type baked into this build. */
export function SellScreen({ session }: { session: Session }) {
  const info = useAppInfo();
  switch (info.data?.client.business_type) {
    case undefined:
      return null;
    case 'retail':
      return <QuickSale session={session} orderType="counter" retail />;
    case 'cafe':
      return <CafeSell session={session} />;
    case 'restaurant':
      return <RestaurantSell session={session} />;
  }
}
