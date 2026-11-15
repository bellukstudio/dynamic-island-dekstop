#!/usr/bin/env python3
"""Membuat notch-core/src/holidays_data.rs dari src/main-modules/holidays.ts (FIXED dan BY_YEAR).
Jalankan dari akar repo bila data bawaan berubah:  python3 scripts/gen_holidays_rs.py
"""
import re
ts = open('src/main-modules/holidays.ts').read()
fixed = re.findall(r'\[(\d+), (\d+), "([^"]+)", "(\w+)"\]', ts)
by = re.search(r'const BY_YEAR.*?= \{(.*?)\n\};', ts, re.S).group(1)
years = {y: re.findall(r'\["(\d\d)-(\d\d)", "([^"]+)", "(\w+)"\]', blk)
         for y, blk in re.findall(r'(\d{4}): \[(.*?)\n    \],', by, re.S)}
K = {'libur': 'Kind::Libur', 'cuti': 'Kind::Cuti', 'hari': 'Kind::Hari'}
out = ["// BERKAS HASIL GENERATOR: jangan edit manual. Sumber: src/main-modules/holidays.ts (FIXED, BY_YEAR).",
       "// Dibuat oleh scripts/gen_holidays_rs.py.", "", "use crate::calendar::Kind;", "",
       "/// Hari libur tetap + hari peringatan nasional yang berlaku setiap tahun: (bulan, tanggal, nama, jenis).",
       "pub static FIXED: &[(u32, u32, &str, Kind)] = &["]
out += [f'    ({m}, {d}, "{n}", {K[k]}),' for m, d, n, k in fixed]
out += ["];", "",
        "/// Libur keagamaan + cuti bersama per tahun (cadangan bila data internet belum ada): (tahun, bulan, tanggal, nama, jenis).",
        "pub static BY_YEAR: &[(i32, u32, u32, &str, Kind)] = &["]
for y, items in years.items():
    out += [f'    ({y}, {int(m)}, {int(d)}, "{n}", {K[k]}),' for m, d, n, k in items]
out.append("];")
open('notch-core/src/holidays_data.rs', 'w').write("\n".join(out) + "\n")
print('ok', len(fixed), {y: len(v) for y, v in years.items()})
