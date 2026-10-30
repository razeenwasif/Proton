// Molecules as the Lab reads them: each kind of file, bonds found from
// places, SMILES laid out, and a protein's helices and strands.
import {test} from 'node:test';
import {equal, deepEqual, ok, throws, match} from 'node:assert/strict';
import {symbol, findBonds, readXyz, readPdb, readCif, readMol, readSmiles, parseSmiles, embed, readMolecule,
  isMolecule, backbone, isProtein, cifTokens, describe, ready} from '../../ui/molecule.js';

const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
const formula = (atoms) => {
  const n = {};
  for (const a of atoms) n[a.el] = (n[a.el] ?? 0) + 1;
  return Object.keys(n).sort().map((k) => `${k}${n[k]}`).join(' ');
};

/** A PDB ATOM line, in its columns. */
function atomLine(serial, name, res, chain, seq, x, y, z, el, {het = false, alt = ' '} = {}) {
  const padName = name.length < 4 && el.length === 1 ? ` ${name}`.padEnd(4) : name.padEnd(4);
  return `${(het ? 'HETATM' : 'ATOM').padEnd(6)}${String(serial).padStart(5)} ${padName}${alt}${res.padStart(3)} ${chain}${String(seq).padStart(4)}    ` +
    `${x.toFixed(3).padStart(8)}${y.toFixed(3).padStart(8)}${z.toFixed(3).padStart(8)}  1.00  0.00          ${el.padStart(2)}`;
}

/** An ideal α-helix's α-carbons and carbonyl oxygens, then a stretched
 *  strand: 3.6 residues a turn, 1.5 Å a residue, 2.3 Å out; 3.3 Å a residue
 *  along a zig-zag. */
function protein({records = false} = {}) {
  const lines = [];
  let serial = 1;
  for (let k = 0; k < 14; k++) {
    const t = (k * 100 * Math.PI) / 180;
    const [x, y, z] = [2.3 * Math.cos(t), 2.3 * Math.sin(t), 1.5 * k];
    lines.push(atomLine(serial++, 'CA', 'ALA', 'A', k + 1, x, y, z, 'C'));
    lines.push(atomLine(serial++, 'O', 'ALA', 'A', k + 1, x * 1.6, y * 1.6, z + 0.6, 'O'));
  }
  for (let k = 0; k < 8; k++) {
    // From the helix's last α-carbon, a peptide bond's length on.
    const [x, y, z] = [-1.76 + 3.3 * k, -1.48 + (k % 2 ? 0.9 : -0.9), 23];
    lines.push(atomLine(serial++, 'CA', 'GLY', 'A', 15 + k, x, y, z, 'C'));
    lines.push(atomLine(serial++, 'O', 'GLY', 'A', 15 + k, x, y + 1.2, z + 1, 'O'));
  }
  const head = records
    ? ['HELIX    1   1 ALA A    1  ALA A   14  1                                  14',
      'SHEET    1   A 1 GLY A  15  GLY A  22  0']
    : [];
  return [...head, ...lines, 'END'].join('\n');
}

test('an element is read in its usual case', () => {
  deepEqual(['CL', 'cl', 'C', 'Fe', 'FE', 'h', '', '12'].map(symbol), ['Cl', 'Cl', 'C', 'Fe', 'Fe', 'H', 'X', 'X']);
});

test('XYZ: frames of a trajectory, bonds found from distance, never between hydrogens', () => {
  const water = (dx) => `3\nwater ${dx}\nO 0 0 0\nH ${0.96 + dx} 0 0\nH ${-0.24} 0.93 0\n`;
  const m = readXyz(water(0) + water(0.5), 'water.xyz');
  equal(m.models.length, 2);
  equal(m.models[1].name, 'water 0.5');
  deepEqual(findBonds(m.models[0].atoms).map(([i, j]) => [i, j]).sort(), [[0, 1], [0, 2]]);
  // Stretched past what oxygen and hydrogen's radii allow, it is no bond.
  equal(findBonds(m.models[1].atoms).length, 1);
  // Atomic numbers instead of symbols.
  equal(readXyz('1\n\n8 0 0 0\n', 'o.xyz').models[0].atoms[0].el, 'O');
  throws(() => readXyz('2\nshort\nO 0 0 0\n', 'x.xyz'), /line 4: an atom is its element and three coordinates/);
  throws(() => readXyz('water\n', 'x.xyz'), /starts with its number of atoms/);
});

test('PDB: columns, alternate places, a calcium told from an α-carbon, HELIX and SHEET', () => {
  const text = [
    'HEADER    TEST PROTEIN',
    atomLine(1, 'CA', 'ALA', 'A', 1, 0, 0, 0, 'C'),
    atomLine(2, 'CB', 'ALA', 'A', 1, 1.53, 0, 0, 'C', {alt: 'A'}),
    atomLine(3, 'CB', 'ALA', 'A', 1, 0, 1.53, 0, 'C', {alt: 'B'}),
    atomLine(4, 'CA', 'CA', 'B', 101, 9, 9, 9, 'CA', {het: true}),
    atomLine(5, 'O', 'HOH', 'B', 201, 11, 9, 9, 'O', {het: true}),
    'CONECT    4    5',
    'END',
  ].join('\n');
  const m = readPdb(text, '');
  equal(m.title, 'TEST PROTEIN');
  const atoms = m.models[0].atoms;
  equal(atoms.length, 4, 'the second place of an atom is left out');
  deepEqual(atoms.map((a) => a.el), ['C', 'C', 'Ca', 'O']);
  equal(atoms[2].het, true);
  // CONECT bonds what distance would not: the calcium to its water.
  ok(m.models[0].bonds.some(([i, j]) => i === 2 && j === 3));
  // Without the element column, the name's alignment says which.
  const bare = (line) => line.slice(0, 76);
  const guessed = readPdb([bare(atomLine(1, 'CA', 'ALA', 'A', 1, 0, 0, 0, 'C')), bare('HETATM    2 CA    CA B 101       9.000   9.000   9.000  1.00  0.00')].join('\n'), '');
  deepEqual(guessed.models[0].atoms.map((a) => a.el), ['C', 'Ca']);
  // MODEL by MODEL, each a frame.
  const two = `MODEL        1\n${atomLine(1, 'N', 'GLY', 'A', 1, 0, 0, 0, 'N')}\nENDMDL\nMODEL        2\n${atomLine(1, 'N', 'GLY', 'A', 1, 1, 0, 0, 'N')}\nENDMDL\n`;
  deepEqual(readPdb(two, '').models.map((x) => x.name), ['Model 1', 'Model 2']);
  throws(() => readPdb('REMARK nothing here\n', ''), /no ATOM or HETATM/);
});

test('a protein\'s helix and strand: from its records, or from its α-carbons when it has none', () => {
  for (const records of [true, false]) {
    const model = readPdb(protein({records}), '').models[0];
    const {pieces, estimated} = backbone(model);
    equal(estimated, !records);
    equal(pieces.length, 1, 'a helix running into a strand is one piece');
    const ss = pieces[0].residues.map((r) => r.ss).join('');
    if (records) {
      equal(ss, 'H'.repeat(14) + 'E'.repeat(8));
    } else {
      match(ss.slice(0, 14), /^H{12,14}/, `the helix was not seen: ${ss}`);
      match(ss.slice(15), /E{5,}/, `the strand was not seen: ${ss}`);
    }
    ok(pieces[0].residues.every((r) => r.o != null), 'each residue has its oxygen, to turn the ribbon by');
    ok(isProtein(model));
  }
  // A gap too long for a peptide bond cuts the chain.
  const gapped = readPdb(protein().replace(/^(ATOM.{24})(.{8})(.{8})(.{8})/gm, (line, head, x, y, z) => {
    const seq = Number(line.slice(22, 26));
    return seq > 7 ? `${head}${(Number(x) + 30).toFixed(3).padStart(8)}${y}${z}` : line;
  }), '').models[0];
  equal(backbone(gapped).pieces.length, 2);
});

test('mmCIF: a loop of atom sites, quoted words and text fields, helices from struct_conf', () => {
  equal(cifTokens("_a 'it''s fine' \"two words\" plain # a comment").join('|'), "_a|it''s fine|two words|plain");
  deepEqual(cifTokens('_title\n;line one\nline two\n;\n'), ['_title', 'line one\nline two']);
  const text = `data_TEST
_struct.title 'A small test'
loop_
_struct_conf.conf_type_id
_struct_conf.beg_auth_asym_id
_struct_conf.beg_auth_seq_id
_struct_conf.end_auth_seq_id
HELX_P A 1 2
loop_
_atom_site.group_PDB
_atom_site.type_symbol
_atom_site.label_atom_id
_atom_site.label_alt_id
_atom_site.label_comp_id
_atom_site.auth_asym_id
_atom_site.auth_seq_id
_atom_site.Cartn_x
_atom_site.Cartn_y
_atom_site.Cartn_z
_atom_site.pdbx_PDB_model_num
ATOM C CA . ALA A 1 0.0 0.0 0.0 1
ATOM C CB A ALA A 1 1.53 0.0 0.0 1
ATOM C CB B ALA A 1 0.0 1.53 0.0 1
HETATM O O . HOH A 9 5.0 5.0 5.0 1
ATOM C CA . ALA A 1 0.5 0.0 0.0 2
`;
  const m = readCif(text, 'x.cif');
  equal(m.title, 'A small test');
  deepEqual(m.models.map((x) => x.atoms.length), [3, 1]);
  deepEqual(m.models[0].bonds.map(([i, j]) => [i, j]), [[0, 1]]);
  deepEqual(m.models[0].helices, [{chain: 'A', from: 1, to: 2}]);
  equal(m.models[0].atoms[2].het, true);
  throws(() => readCif('data_x\n_cell.length_a 5\n', 'x.cif'), /no _atom_site/);
});

test('MOL and SDF: the bond block\'s orders, a molecule a frame', () => {
  const ethene = (name) => `${name}\n  test\n\n  6  5  0  0  0  0  0  0  0  0999 V2000
    0.0000    0.0000    0.0000 C   0  0
    1.3300    0.0000    0.0000 C   0  0
   -0.5600    0.9300    0.0000 H   0  0
   -0.5600   -0.9300    0.0000 H   0  0
    1.8900    0.9300    0.0000 H   0  0
    1.8900   -0.9300    0.0000 H   0  0
  1  2  2  0
  1  3  1  0
  1  4  1  0
  2  5  1  0
  2  6  1  0
M  END
`;
  const one = readMol(ethene('ethene'), '');
  equal(one.kind, 'MOL');
  equal(one.title, 'ethene');
  deepEqual(one.models[0].bonds[0], [0, 1, 2]);
  const sdf = readMol(`${ethene('a')}$$$$\n${ethene('b')}$$$$\n`, '');
  deepEqual([sdf.kind, sdf.models.map((m) => m.name)], ['SDF', ['a', 'b']]);
  throws(() => readMol('x\n\n\n  0  0  0  0  0  0            999 V3000\n', ''), /V3000/);
});

test('SMILES: atoms, branches, rings, charges and the hydrogens left unwritten', () => {
  const ethanol = embed(parseSmiles('CCO'));
  equal(formula(ethanol.atoms), 'C2 H6 O1');
  equal(formula(embed(parseSmiles('c1ccccc1')).atoms), 'C6 H6');
  equal(formula(embed(parseSmiles('Cn1cnc2c1c(=O)n(C)c(=O)n2C')).atoms), 'C8 H10 N4 O2', 'caffeine');
  equal(formula(embed(parseSmiles('[NH4+].[Cl-]')).atoms), 'Cl1 H4 N1');
  equal(formula(embed(parseSmiles('c1cc[nH]c1')).atoms), 'C4 H5 N1', 'pyrrole');
  equal(formula(embed(parseSmiles('OS(=O)(=O)O')).atoms), 'H2 O4 S1', 'sulfuric acid');
  equal(formula(embed(parseSmiles('C%10CCCCC%10')).atoms), 'C6 H12');
  const ring = parseSmiles('C1=CC=CC=C1');
  deepEqual(ring.bonds.map((b) => b[2]), [2, 1, 2, 1, 2, 1]);
  equal(parseSmiles('[13CH3-]').atoms[0].charge, -1);
  throws(() => parseSmiles('C1CC'), /ring bond 1 is never closed/);
  throws(() => parseSmiles('CC(C'), /a \( is never closed/);
  throws(() => parseSmiles('C[Xx]'), /at 2: Xx is not an element/);
  throws(() => parseSmiles('C)C'), /at 2: a \) with no \(/);
  throws(() => parseSmiles('CQ'), /at 2: “Q” is not part of SMILES/);
});

test('SMILES laid out: bonds their length, a benzene ring flat, the same every time', () => {
  const benzene = embed(parseSmiles('c1ccccc1'));
  const carbons = benzene.atoms.slice(0, 6);
  for (let k = 0; k < 6; k++) {
    const d = dist(carbons[k], carbons[(k + 1) % 6]);
    ok(Math.abs(d - 1.40) < 0.05, `C–C ${d.toFixed(3)} Å`);
  }
  // Flat: every atom, hydrogens too, near the plane through three carbons.
  const [a, b, c] = carbons;
  const u = [b.x - a.x, b.y - a.y, b.z - a.z], v = [c.x - a.x, c.y - a.y, c.z - a.z];
  const nrm = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
  const len = Math.hypot(...nrm);
  for (const p of benzene.atoms) {
    const off = Math.abs((p.x - a.x) * nrm[0] + (p.y - a.y) * nrm[1] + (p.z - a.z) * nrm[2]) / len;
    ok(off < 0.08, `an atom ${off.toFixed(3)} Å off the ring's plane`);
  }
  const ethane = embed(parseSmiles('CC'));
  ok(Math.abs(dist(ethane.atoms[0], ethane.atoms[1]) - 1.52) < 0.03);
  // No two atoms on top of each other in something bigger.
  const big = embed(parseSmiles('CC(C)Cc1ccc(cc1)C(C)C(=O)O'));
  for (let i = 0; i < big.atoms.length; i++) for (let j = i + 1; j < big.atoms.length; j++) {
    ok(dist(big.atoms[i], big.atoms[j]) > 0.85, `atoms ${i} and ${j} overlap`);
  }
  deepEqual(embed(parseSmiles('CCO')), embed(parseSmiles('CCO')));
  const file = readSmiles('CCO ethanol\n# a comment\nc1ccccc1\n', '');
  deepEqual(file.models.map((m) => m.name), ['ethanol', 'c1ccccc1']);
  equal(file.models[1].atoms.length, 0, 'laid out only when shown');
  equal(formula(ready(file.models[1]).atoms), 'C6 H6');
  equal(ready(readXyz('2\n\nH 0 0 0\nH 0.74 0 0\n', 'h2.xyz').models[0]).bonds.length, 0);
  throws(() => readSmiles('CCO\nC1CC\n', ''), /line 2, at 4: ring bond 1/);
});

test('one door: a file read by its ending, its name kept when it gives none', () => {
  ok(isMolecule('out/1abc.PDB') && isMolecule('m.smi') && !isMolecule('scene.json'));
  equal(readMolecule('runs/caffeine.smi', 'Cn1cnc2c1c(=O)n(C)c(=O)n2C').title, 'Cn1cnc2c1c(=O)n(C)c(=O)n2C');
  equal(readMolecule('a/b/water.xyz', '1\n\nO 0 0 0\n').title, 'water.xyz');
  throws(() => readMolecule('x.txt', ''), /not a molecule file/);
  equal(describe({el: 'C', x: 1, y: 2, z: 3, name: 'CA', res: 'ALA', seq: 42, chain: 'A'}, 0), 'CA (carbon) · ALA 42, chain A · 1.00, 2.00, 3.00 Å');
  equal(describe({el: 'O', x: 0, y: 0, z: 0}, 2), 'O3 (oxygen) · 0.00, 0.00, 0.00 Å');
});
