import Mathlib

theorem H004 (n : ℕ) : Even (n * (n + 1)) := by
  induction n with
  | zero => simp
  | succ k ih => ring_nf
