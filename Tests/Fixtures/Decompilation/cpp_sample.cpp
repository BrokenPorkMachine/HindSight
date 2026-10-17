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

extern "C" __attribute__((noinline)) int invoke_score(const ScoreBase *value, int input) {
    return value->score(input);
}

int main() {
    ScoreDerived value;
    return invoke_score(&value, 9);
}
