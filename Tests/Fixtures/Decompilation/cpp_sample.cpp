// Original test fixture; released into the public domain.
class ScoreBase {
public:
    virtual ~ScoreBase() = default;
    virtual int score(int value) const = 0;
};

class ScoreDerived final : public ScoreBase {
public:
    int score(int value) const override { return value < 0 ? -value : value * 5; }
};

class QualifiedMethods {
public:
    QualifiedMethods(int value);
    ~QualifiedMethods();
    int measure(int value) const;
    int read() volatile &;
    int consume() const &&;
    int operator+(int value) const;
    int operator[](int index);
};

__attribute__((noinline, used)) QualifiedMethods::QualifiedMethods(int value) { (void)value; }
__attribute__((noinline, used)) QualifiedMethods::~QualifiedMethods() {}
__attribute__((noinline, used)) int QualifiedMethods::measure(int value) const { return value + 1; }
__attribute__((noinline, used)) int QualifiedMethods::read() volatile & { return 2; }
__attribute__((noinline, used)) int QualifiedMethods::consume() const && { return 3; }
__attribute__((noinline, used)) int QualifiedMethods::operator+(int value) const { return value + 4; }
__attribute__((noinline, used)) int QualifiedMethods::operator[](int index) { return index + 5; }
extern "C" __attribute__((noinline, used)) int invoke_operator(const QualifiedMethods *object, int value) {
    return (*object + value) + 1;
}
extern "C" __attribute__((noinline, used)) int tail_operator(const QualifiedMethods *object, int value) {
    return *object + value;
}

extern "C" __attribute__((noinline)) int invoke_score(const ScoreBase *value, int input) {
    return value->score(input);
}

__attribute__((noinline, used)) int scalar_types(wchar_t a, char16_t b, char32_t c, long double d, __int128 e) {
    return int(a) + int(b) + int(c) + int(d) + int(e);
}

int main() {
    ScoreDerived value;
    return invoke_score(&value, 9);
}
