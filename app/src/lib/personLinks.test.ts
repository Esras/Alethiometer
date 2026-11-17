import { describe, expect, it } from 'vitest';
import { atQuery, insertLink, matchPeople, personLink, personOf, wikilink } from './personLinks';

const kim = { name: 'Kim Lee', note_path: 'People/Kim Lee.md' };
const jo = { name: 'Jordan Kimura', note_path: 'People/Jordan Kimura.md' };
const people = [jo, kim];

describe('person links in notes', () => {
  it('writes the vault path form', () => expect(personLink(kim)).toBe('[[People/Kim Lee]]'));
  it('reads a link, its alias and heading', () => {
    expect(wikilink('[[People/Kim Lee]]')).toEqual({ target: 'People/Kim Lee', shown: 'Kim Lee' });
    expect(wikilink('[[People/Kim Lee|Kim]]')).toEqual({ target: 'People/Kim Lee', shown: 'Kim' });
    expect(wikilink('[[People/Kim Lee#Notes]]').target).toBe('People/Kim Lee');
  });
  it('finds the person by path or by name', () => {
    expect(personOf('people/kim lee', people)).toBe(kim);
    expect(personOf('Kim Lee', people)).toBe(kim);
    expect(personOf('Projects/Kim Lee', people)).toBeUndefined();
  });
  it('sees an @word being typed, not an email', () => {
    expect(atQuery('ping @Ki', 8)).toEqual({ start: 5, q: 'Ki' });
    expect(atQuery('@', 1)).toEqual({ start: 0, q: '' });
    expect(atQuery('mail kim@work', 13)).toBeNull();
    expect(atQuery('@kim lee', 8)).toBeNull();
  });
  it('matches a word of the name, first names first', () => {
    expect(matchPeople(people, 'kim')).toEqual([kim, jo]);
    expect(matchPeople(people, 'lee')).toEqual([kim]);
  });
  it('replaces the @word with the link', () => {
    expect(insertLink('ping @Ki today', 5, 8, '[[People/Kim Lee]]')).toEqual({
      text: 'ping [[People/Kim Lee]] today',
      caret: 23,
    });
  });
});
