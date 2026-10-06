import unittest

import pandas as pd

from e1_analyze_20260929 import BASE_CIRCUITS, STRICT_COLUMNS, validate_grid


class GridValidationTests(unittest.TestCase):
    def grid(self):
        return pd.DataFrame([
            [circuit, events, seed, 0.1, 1.0, 1000, 0.1]
            for circuit in BASE_CIRCUITS for events in [8, 16] for seed in [1, 2]
        ], columns=STRICT_COLUMNS)

    def validate(self, df):
        validate_grid(df, 2, [8, 16], BASE_CIRCUITS)

    def test_complete_grid(self):
        self.validate(self.grid())

    def test_missing_entire_cell(self):
        df = self.grid()
        with self.assertRaisesRegex(SystemExit, "missing="):
            self.validate(df[~((df.circuit == BASE_CIRCUITS[0]) & (df.events_per_lot == 8))])

    def test_duplicate_and_wrong_seed(self):
        df = self.grid()
        with self.assertRaisesRegex(SystemExit, "Duplicate"):
            self.validate(pd.concat([df, df.iloc[:1]]))
        df.loc[0, "seed"] = 3
        with self.assertRaisesRegex(SystemExit, "Invalid seed"):
            self.validate(df)

    def test_invalid_measurement(self):
        for value in [float("nan"), float("inf"), 0, -1]:
            df = self.grid()
            df.loc[0, "prove_time_s"] = value
            with self.assertRaises(SystemExit):
                self.validate(df)


if __name__ == "__main__":
    unittest.main()
