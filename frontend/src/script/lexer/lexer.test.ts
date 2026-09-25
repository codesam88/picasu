import { describe, expect, test } from 'vitest'
import { MyLexer, MyParser, MyVisitor } from './lexer'

// One parser + visitor pair for the whole suite; chevrotain instances are reusable.
const parser = new MyParser()
const visitor = new MyVisitor()

function parse(input: string): unknown {
  const { tokens } = MyLexer.tokenize(input)
  parser.input = tokens
  return visitor.visit(parser.expression())
}

// ── Atomic expressions ────────────────────────────────────────────────────────

describe('tag', () => {
  test('string value', () => {
    expect(parse('tag:"holiday"')).toEqual({ Tag: 'holiday' })
  })
  test('boolean true', () => {
    expect(parse('tag:true')).toEqual({ Tag: true })
  })
  test('boolean false', () => {
    expect(parse('tag:false')).toEqual({ Tag: false })
  })
})

describe('type', () => {
  test('image', () => {
    expect(parse('type:"image"')).toEqual({ ExtType: 'image' })
  })
  test('video', () => {
    expect(parse('type:"video"')).toEqual({ ExtType: 'video' })
  })
})

describe('ext', () => {
  test('jpg', () => {
    expect(parse('ext:"jpg"')).toEqual({ Ext: 'jpg' })
  })
})

describe('make', () => {
  test('string value', () => {
    expect(parse('make:"Canon"')).toEqual({ Make: 'Canon' })
  })
  test('boolean', () => {
    expect(parse('make:true')).toEqual({ Make: true })
  })
})

describe('model', () => {
  test('string value', () => {
    expect(parse('model:"EOS R5"')).toEqual({ Model: 'EOS R5' })
  })
  test('boolean', () => {
    expect(parse('model:false')).toEqual({ Model: false })
  })
})

describe('album', () => {
  test('string value', () => {
    expect(parse('album:"Summer 2024"')).toEqual({ Album: 'Summer 2024' })
  })
  test('boolean', () => {
    expect(parse('album:true')).toEqual({ Album: true })
  })
})

describe('path', () => {
  test('path value', () => {
    expect(parse('path:"/photos/2024"')).toEqual({ Path: '/photos/2024' })
  })
})

describe('parent_album', () => {
  test('string id', () => {
    expect(parse('parent_album:"abc123"')).toEqual({ ParentAlbum: 'abc123' })
  })
})

describe('any', () => {
  test('any value', () => {
    expect(parse('any:"keyword"')).toEqual({ Any: 'keyword' })
  })
})

// The favorite/archived flags were removed together with the object fields
// they filtered, so their query keywords must not survive in the language.
describe('removed status keywords', () => {
  test('favorite is rejected by the lexer', () => {
    const { errors } = MyLexer.tokenize('favorite:true')
    expect(errors.length).toBeGreaterThan(0)
  })
  test('archived is rejected by the lexer', () => {
    const { errors } = MyLexer.tokenize('archived:true')
    expect(errors.length).toBeGreaterThan(0)
  })
  test('a query mixing them does not parse', () => {
    const { tokens } = MyLexer.tokenize('and(favorite:true, trashed:false)')
    parser.input = tokens
    parser.expression()
    expect(parser.errors.length).toBeGreaterThan(0)
  })
})

describe('trashed', () => {
  test('true', () => {
    expect(parse('trashed:true')).toEqual({ Trashed: true })
  })
  test('false', () => {
    expect(parse('trashed:false')).toEqual({ Trashed: false })
  })
})

// ── Compound expressions ──────────────────────────────────────────────────────

describe('not', () => {
  test('wraps inner expression', () => {
    expect(parse('not(trashed:true)')).toEqual({ Not: { Trashed: true } })
  })
  test('double negation', () => {
    expect(parse('not(not(trashed:false))')).toEqual({ Not: { Not: { Trashed: false } } })
  })
})

describe('and', () => {
  test('two terms', () => {
    expect(parse('and(tag:"a", tag:"b")')).toEqual({ And: [{ Tag: 'a' }, { Tag: 'b' }] })
  })
  test('three terms', () => {
    expect(parse('and(type:"image", trashed:false, root_album:false)')).toEqual({
      And: [{ ExtType: 'image' }, { Trashed: false }, { RootAlbum: false }]
    })
  })
  test('nested inside not', () => {
    expect(parse('not(and(type:"video", trashed:true))')).toEqual({
      Not: { And: [{ ExtType: 'video' }, { Trashed: true }] }
    })
  })
})

describe('or', () => {
  test('two terms', () => {
    expect(parse('or(type:"image", type:"video")')).toEqual({
      Or: [{ ExtType: 'image' }, { ExtType: 'video' }]
    })
  })
  test('three terms', () => {
    expect(parse('or(ext:"jpg", ext:"png", ext:"webp")')).toEqual({
      Or: [{ Ext: 'jpg' }, { Ext: 'png' }, { Ext: 'webp' }]
    })
  })
  test('nested inside and', () => {
    expect(parse('and(trashed:false, or(type:"image", type:"video"))')).toEqual({
      And: [{ Trashed: false }, { Or: [{ ExtType: 'image' }, { ExtType: 'video' }] }]
    })
  })
})

// ── String escaping (unescapeAndUnwrap) ───────────────────────────────────────

describe('string escaping', () => {
  test('escaped double quote inside value', () => {
    // query: tag:"it\"s"  →  Tag: it"s
    expect(parse('tag:"it\\"s"')).toEqual({ Tag: 'it"s' })
  })
  test('escaped backslash inside value', () => {
    // query: tag:"back\\slash"  →  Tag: back\slash
    expect(parse('tag:"back\\\\slash"')).toEqual({ Tag: 'back\\slash' })
  })
  test('spaces inside quoted value are preserved', () => {
    expect(parse('tag:"hello world"')).toEqual({ Tag: 'hello world' })
  })
  test('empty string', () => {
    expect(parse('tag:""')).toEqual({ Tag: '' })
  })
})

// ── Lexer error surface ───────────────────────────────────────────────────────

describe('lexer errors', () => {
  test('unrecognised token produces a lex error', () => {
    const { errors } = MyLexer.tokenize('tag:unquoted')
    expect(errors.length).toBeGreaterThan(0)
  })
  test('unclosed string produces a lex error', () => {
    const { errors } = MyLexer.tokenize('tag:"unclosed')
    expect(errors.length).toBeGreaterThan(0)
  })
})
