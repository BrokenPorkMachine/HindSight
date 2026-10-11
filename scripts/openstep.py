"""Small structural reader for Xcode's OpenStep project format."""
import json
import re

def load_project(text):
    tokens = [
        token for token in re.findall(
            r'"(?:\\.|[^"\\])*"|/\*.*?\*/|//[^\n]*|[{}()=;,]|[^\s{}()=;,"]+',
            text,
            re.S,
        )
        if not token.startswith(('/*', '//'))
    ]
    position = 0
    def pop(expected=None):
        nonlocal position
        if position >= len(tokens): raise ValueError('Unexpected end of project')
        value=tokens[position];position+=1
        if expected is not None and value != expected: raise ValueError(f'Expected {expected}, got {value}')
        return value
    def atom(token):
        try:
            return json.loads(token)
        except json.JSONDecodeError:
            # OpenStep permits unquoted keys and atoms such as YES, NO, and SDKROOT names.
            return token
    def value():
        token=pop()
        if token=='{':
            result={}
            while tokens[position]!='}':
                key=atom(pop());pop('=');item=value();pop(';')
                if key in result: raise ValueError('Duplicate project key')
                result[key]=item
            pop('}');return result
        if token=='(':
            result=[]
            while tokens[position]!=')':
                result.append(value())
                if tokens[position]==',': pop(',')
                elif tokens[position]!=')': raise ValueError('Expected list separator')
            pop(')');return result
        return atom(token)
    result=value()
    if position!=len(tokens): raise ValueError('Trailing project data')
    return result
