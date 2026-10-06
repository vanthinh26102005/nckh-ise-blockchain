#!/usr/bin/env python3
"""Estimate execution-gas USD cost from measured gas and explicit dated assumptions."""
import argparse
import json
from datetime import date
from decimal import Decimal, InvalidOperation, localcontext


def positive_decimal(raw):
    try:
        value = Decimal(raw)
    except InvalidOperation as exc:
        raise argparse.ArgumentTypeError('must be a positive finite decimal') from exc
    if not value.is_finite() or value <= 0:
        raise argparse.ArgumentTypeError('must be a positive finite decimal')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gas-used', required=True, type=positive_decimal)
    for scenario in ['low', 'mid', 'high']:
        parser.add_argument(f'--gas-gwei-{scenario}', required=True, type=positive_decimal)
    parser.add_argument('--eth-usd', required=True, type=positive_decimal)
    parser.add_argument('--date', required=True, type=date.fromisoformat)
    parser.add_argument('--source', required=True, help='source URL or assumption citation for gas and ETH prices')
    args = parser.parse_args()
    if args.gas_used != args.gas_used.to_integral_value():
        parser.error('--gas-used must be an integer receipt gas count')
    if not args.source.strip():
        parser.error('--source must not be blank')
    prices = [args.gas_gwei_low, args.gas_gwei_mid, args.gas_gwei_high]
    if prices != sorted(prices):
        parser.error('gas prices must satisfy low <= mid <= high')
    scenarios = []
    with localcontext() as context:
        context.prec = max(50, len(args.gas_used.as_tuple().digits) + len(args.eth_usd.as_tuple().digits) + max(len(p.as_tuple().digits) for p in prices))
        for name, price in zip(['low', 'mid', 'high'], prices):
            eth = args.gas_used * price * Decimal('0.000000001')
            scenarios.append({'scenario': name, 'gas_gwei': str(price), 'eth': str(eth), 'usd': format((eth * args.eth_usd).normalize(), 'f')})
    print(json.dumps({'kind': 'gas-to-USD execution cost estimate', 'scope': 'estimate under supplied assumptions; not actual paid mainnet fees; excludes separate L1/L2 data fees', 'gas_used': str(args.gas_used), 'eth_usd': str(args.eth_usd), 'assumptions_date': args.date.isoformat(), 'source': args.source.strip(), 'formula': 'gas_used * gas_gwei * 1e-9 * ETH_USD', 'scenarios': scenarios}, indent=2))


if __name__ == '__main__':
    main()
