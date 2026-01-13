## Description

<!-- Provide a brief description of the changes in this PR -->

## Type of Change

<!-- Mark the relevant option with an 'x' -->

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)
- [ ] Performance improvement
- [ ] Database migration

## Checklist

- [ ] My code follows the project's style guidelines
- [ ] I have performed a self-review of my own code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] I have made corresponding changes to the documentation
- [ ] My changes generate no new warnings
- [ ] I have added tests that prove my fix is effective or that my feature works
- [ ] New and existing unit tests pass locally with my changes
- [ ] Any dependent changes have been merged and published

## Database Changes

<!-- If this PR includes database migrations, fill out this section -->

- [ ] This PR includes database migrations
- [ ] Migrations have been tested locally
- [ ] Migration rollback has been tested (if applicable)
- [ ] SQLx offline data has been regenerated (`cargo sqlx prepare`)
- [ ] `.sqlx/` directory has been committed

## Testing

<!-- Describe the tests you ran to verify your changes -->

```bash
# Commands used to test
cargo test
cargo clippy
```

## Screenshots (if applicable)

<!-- Add screenshots here if your changes affect the UI -->

## Additional Notes

<!-- Any additional information that reviewers should know -->
